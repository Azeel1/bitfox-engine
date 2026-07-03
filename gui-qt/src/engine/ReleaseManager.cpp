#include "engine/ReleaseManager.h"

#include <QCoreApplication>
#include <QDir>
#include <QDirIterator>
#include <QFile>
#include <QFileInfo>
#include <QJsonArray>
#include <QJsonDocument>
#include <QJsonObject>
#include <QNetworkAccessManager>
#include <QNetworkReply>
#include <QNetworkRequest>
#include <QProcess>
#include <QStandardPaths>
#include <QSysInfo>
#include <QUrl>

#include <memory>

static const char *kReleasesUrl =
    "https://api.github.com/repos/Azeel1/bitfox-engine/releases";

ReleaseManager::ReleaseManager(QObject *parent)
    : QObject(parent), m_net(new QNetworkAccessManager(this)) {}

QString ReleaseManager::enginesDir() {
    return QCoreApplication::applicationDirPath() + "/engines";
}

QString ReleaseManager::platformAssetSuffix() {
#if defined(Q_OS_MACOS)
    return QSysInfo::currentCpuArchitecture() == "arm64" ? "-macos-arm64.tar.gz"
                                                          : "-macos-x86_64.tar.gz";
#elif defined(Q_OS_WIN)
    return "-windows-x86_64.zip";
#elif defined(Q_OS_LINUX)
    const QString arch = QSysInfo::currentCpuArchitecture();
    return (arch.contains("arm") || arch.contains("aarch")) ? "-linux-aarch64.tar.gz"
                                                              : "-linux-x86_64.tar.gz";
#else
    return QString();
#endif
}

bool ReleaseManager::isInstalled(const QString &tag) {
    QString version = tag;
    if (version.startsWith('v')) {
        version.remove(0, 1);
    }
#if defined(Q_OS_WIN)
    const QString exe = QDir(enginesDir()).filePath("Bitfox " + version + ".exe");
#else
    const QString exe = QDir(enginesDir()).filePath("Bitfox " + version);
#endif
    return QFileInfo::exists(exe);
}

void ReleaseManager::fetchReleases() {
    QNetworkRequest req{QUrl(kReleasesUrl)};
    req.setRawHeader("Accept", "application/vnd.github+json");
    req.setRawHeader("User-Agent", "Bitfox-GUI");
    QNetworkReply *reply = m_net->get(req);
    connect(reply, &QNetworkReply::finished, this, [this, reply] {
        reply->deleteLater();
        if (reply->error() != QNetworkReply::NoError) {
            emit fetchFailed(reply->errorString());
            return;
        }
        const QJsonDocument doc = QJsonDocument::fromJson(reply->readAll());
        if (!doc.isArray()) {
            emit fetchFailed(tr("unexpected response from GitHub"));
            return;
        }
        const QString suffix = platformAssetSuffix();
        QVector<ReleaseInfo> releases;
        for (const QJsonValue &v : doc.array()) {
            const QJsonObject obj = v.toObject();
            ReleaseInfo info;
            info.tag = obj.value("tag_name").toString();
            info.name = obj.value("name").toString();
            if (info.name.isEmpty()) {
                info.name = info.tag;
            }
            for (const QJsonValue &av : obj.value("assets").toArray()) {
                const QJsonObject asset = av.toObject();
                const QString assetName = asset.value("name").toString();
                if (!suffix.isEmpty() && assetName.endsWith(suffix) &&
                    !assetName.contains("-gui")) {
                    info.assetName = assetName;
                    info.assetUrl = asset.value("browser_download_url").toString();
                    break;
                }
            }
            if (info.assetUrl.isEmpty()) {
                continue;
            }
            info.installed = isInstalled(info.tag);
            releases.append(info);
        }
        emit releasesReady(releases);
    });
}

void ReleaseManager::downloadRelease(const ReleaseInfo &info) {
    if (info.assetUrl.isEmpty()) {
        emit downloadFinished(info.tag, false, tr("no build available for this platform"));
        return;
    }
    if (m_inFlight.contains(info.tag)) {
        return;
    }
    m_inFlight.insert(info.tag);

    QNetworkRequest req{QUrl(info.assetUrl)};
    req.setRawHeader("User-Agent", "Bitfox-GUI");
    QNetworkReply *reply = m_net->get(req);
    const QString tag = info.tag;
    const QString assetName = info.assetName;

    connect(reply, &QNetworkReply::downloadProgress, this,
            [this, tag](qint64 received, qint64 total) {
                emit downloadProgress(tag, received, total);
            });

    connect(reply, &QNetworkReply::finished, this, [this, reply, tag, assetName] {
        reply->deleteLater();
        const QString tmpDir = QStandardPaths::writableLocation(QStandardPaths::TempLocation) +
                                "/bitfox-download-" + tag;
        if (reply->error() != QNetworkReply::NoError) {
            fail(tag, tmpDir, reply->errorString());
            return;
        }
        QDir().mkpath(tmpDir);
        const QString archivePath = QDir(tmpDir).filePath(assetName);
        QFile out(archivePath);
        if (!out.open(QIODevice::WriteOnly)) {
            fail(tag, tmpDir, tr("could not write temporary file"));
            return;
        }
        out.write(reply->readAll());
        out.close();
        handleDownloadedArchive(tag, archivePath);
    });
}

void ReleaseManager::fail(const QString &tag, const QString &archiveDir, const QString &error) {
    if (!archiveDir.isEmpty()) {
        QDir(archiveDir).removeRecursively();
    }
    m_inFlight.remove(tag);
    emit downloadFinished(tag, false, error);
}

void ReleaseManager::handleDownloadedArchive(const QString &tag, const QString &archivePath) {
    const QFileInfo archiveInfo(archivePath);
    const QString archiveDir = archiveInfo.absolutePath();
    const QString extractDir = archiveDir + "/extracted";
    QDir().mkpath(extractDir);

    auto *proc = new QProcess(this);
#if defined(Q_OS_WIN)
    proc->start("tar", {"-xf", archivePath, "-C", extractDir});
#else
    if (archivePath.endsWith(".zip")) {
        proc->start("unzip", {"-o", archivePath, "-d", extractDir});
    } else {
        proc->start("tar", {"-xzf", archivePath, "-C", extractDir});
    }
#endif
    auto settled = std::make_shared<bool>(false);
    connect(proc, &QProcess::errorOccurred, this,
            [this, proc, tag, archiveDir, settled](QProcess::ProcessError err) {
                if (*settled || err != QProcess::FailedToStart) {
                    return;
                }
                *settled = true;
                proc->deleteLater();
                fail(tag, archiveDir, tr("could not run archive extractor"));
            });
    connect(proc, QOverload<int, QProcess::ExitStatus>::of(&QProcess::finished), this,
            [this, proc, tag, archiveDir, extractDir, settled](int exitCode,
                                                                 QProcess::ExitStatus status) {
                if (*settled) {
                    return;
                }
                *settled = true;
                proc->deleteLater();
                if (status != QProcess::NormalExit || exitCode != 0) {
                    fail(tag, archiveDir, tr("could not extract downloaded archive"));
                    return;
                }
                installExtractedBinary(tag, archiveDir, extractDir);
            });
}

void ReleaseManager::installExtractedBinary(const QString &tag, const QString &archiveDir,
                                             const QString &extractDir) {
    QString binaryPath;
    QDirIterator it(extractDir, QDir::Files, QDirIterator::Subdirectories);
    while (it.hasNext()) {
        const QString path = it.next();
        const QFileInfo fi(path);
#if defined(Q_OS_WIN)
        if (fi.fileName().compare("bitfox.exe", Qt::CaseInsensitive) == 0) {
#else
        if (fi.fileName() == "bitfox") {
#endif
            binaryPath = path;
            break;
        }
    }
    if (binaryPath.isEmpty()) {
        fail(tag, archiveDir, tr("engine binary not found in archive"));
        return;
    }

    QString version = tag;
    if (version.startsWith('v')) {
        version.remove(0, 1);
    }
    QDir().mkpath(enginesDir());
#if defined(Q_OS_WIN)
    const QString dest = QDir(enginesDir()).filePath("Bitfox " + version + ".exe");
#else
    const QString dest = QDir(enginesDir()).filePath("Bitfox " + version);
#endif
    const QString tmpDest = dest + ".download";
    QFile::remove(tmpDest);
    if (!QFile::copy(binaryPath, tmpDest)) {
        fail(tag, archiveDir, tr("could not install engine binary"));
        return;
    }
    QFile::setPermissions(tmpDest, QFile::permissions(tmpDest) | QFileDevice::ExeOwner |
                                        QFileDevice::ExeGroup | QFileDevice::ExeOther);
    QFile::remove(dest);
    if (!QFile::rename(tmpDest, dest)) {
        QFile::remove(tmpDest);
        fail(tag, archiveDir, tr("could not install engine binary"));
        return;
    }

    QDir(archiveDir).removeRecursively();
    m_inFlight.remove(tag);
    emit downloadFinished(tag, true, QString());
}
