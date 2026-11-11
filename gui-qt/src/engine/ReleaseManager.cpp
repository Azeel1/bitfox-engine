#include "engine/ReleaseManager.h"

#include <QCoreApplication>
#include <QCryptographicHash>
#include <QDir>
#include <QElapsedTimer>
#include <QFile>
#include <QFileInfo>
#include <QJsonArray>
#include <QJsonDocument>
#include <QJsonObject>
#include <QMap>
#include <QNetworkAccessManager>
#include <QNetworkReply>
#include <QNetworkRequest>
#include <QProcess>
#include <QProcessEnvironment>
#include <QRegularExpression>
#include <QSaveFile>
#include <QStandardPaths>
#include <QSysInfo>
#include <QTemporaryDir>
#include <QTimer>
#include <QUrl>

#include <memory>

namespace {
const char *kReleasesUrl = "https://api.github.com/repos/Azeel1/bitfox-engine/releases";
constexpr qint64 kMaxArchiveBytes = 256LL * 1024 * 1024;
constexpr qsizetype kMaxArchiveListingBytes = 1024 * 1024;
constexpr qsizetype kMaxReleaseResponseBytes = 4 * 1024 * 1024;
constexpr qsizetype kMaxChecksumBytes = 1024;
constexpr qint64 kStreamChunkBytes = 64 * 1024;

enum class StreamResult { Ok, TooLarge, WriteFailed };

bool appendDeviceData(QIODevice *device, QByteArray *body, qsizetype limit) {
    while (device->bytesAvailable() > 0) {
        const qsizetype remaining = limit - body->size();
        const qint64 requested =
            qMin<qint64>(kStreamChunkBytes, static_cast<qint64>(remaining) + 1);
        const QByteArray chunk = device->read(requested);
        if (chunk.isEmpty() || chunk.size() > remaining) {
            return false;
        }
        body->append(chunk);
    }
    return true;
}

StreamResult writeDeviceData(QIODevice *device, QFile *file, QCryptographicHash *hash,
                             qint64 *written, qint64 limit = kMaxArchiveBytes) {
    while (device->bytesAvailable() > 0) {
        const qint64 remaining = limit - *written;
        const qint64 requested = qMin(kStreamChunkBytes, remaining + 1);
        const QByteArray chunk = device->read(requested);
        if (chunk.isEmpty()) {
            return StreamResult::WriteFailed;
        }
        if (chunk.size() > remaining) {
            return StreamResult::TooLarge;
        }
        if (file->write(chunk) != chunk.size()) {
            return StreamResult::WriteFailed;
        }
        if (hash) {
            hash->addData(chunk);
        }
        *written += chunk.size();
    }
    return StreamResult::Ok;
}

void armTotalTimeout(QNetworkReply *reply, int timeoutMs) {
    auto *timer = new QTimer(reply);
    timer->setSingleShot(true);
    QObject::connect(timer, &QTimer::timeout, reply, [reply] {
        reply->setProperty("bitfoxTimedOut", true);
        reply->abort();
    });
    QObject::connect(reply, &QNetworkReply::finished, timer, &QTimer::stop);
    timer->start(timeoutMs);
}

QString networkError(QNetworkReply *reply) {
    if (reply->property("bitfoxUntrustedRedirect").toBool()) {
        return QCoreApplication::translate("ReleaseManager", "untrusted network redirect");
    }
    return reply->property("bitfoxTimedOut").toBool()
               ? QCoreApplication::translate("ReleaseManager", "network request timed out")
               : reply->errorString();
}

bool trustedResponseUrl(const QUrl &url) {
    const QString host = url.host().toLower();
    return url.isValid() && url.scheme() == QLatin1String("https") &&
           (url.port() == -1 || url.port() == 443) && url.userInfo().isEmpty() &&
           (host == QLatin1String("api.github.com") || host == QLatin1String("github.com") ||
            host.endsWith(QLatin1String(".githubusercontent.com")));
}

void guardRedirects(QNetworkReply *reply) {
    QObject::connect(reply, &QNetworkReply::redirected, reply, [reply](const QUrl &url) {
        if (!trustedResponseUrl(url)) {
            reply->setProperty("bitfoxUntrustedRedirect", true);
            reply->abort();
        }
    });
}

QString archiveTool() {
#if defined(Q_OS_WIN)
    const QString systemRoot = qEnvironmentVariable("SystemRoot");
    if (systemRoot.isEmpty()) {
        return QString();
    }
    return QStandardPaths::findExecutable(
        QStringLiteral("tar.exe"), {QDir(systemRoot).filePath(QStringLiteral("System32"))});
#else
    return QStandardPaths::findExecutable(QStringLiteral("tar"),
                                          {QStringLiteral("/usr/bin"), QStringLiteral("/bin")});
#endif
}

QNetworkRequest requestFor(const QString &url) {
    QNetworkRequest request{QUrl(url)};
    request.setRawHeader("Accept", "application/vnd.github+json");
    request.setRawHeader("User-Agent", "Bitfox-GUI");
    request.setTransferTimeout(30000);
    request.setAttribute(QNetworkRequest::RedirectPolicyAttribute,
                         QNetworkRequest::NoLessSafeRedirectPolicy);
    request.setMaximumRedirectsAllowed(5);
    return request;
}

void configureArchiveProcess(QProcess *proc) {
    QProcessEnvironment env = QProcessEnvironment::systemEnvironment();
    static const char *variables[] = {"TAR_OPTIONS", "TAPE", "GZIP", "GZIP_OPT",
                                      "BZIP",        "BZIP2", "XZ_OPT", "POSIXLY_CORRECT"};
    for (const char *name : variables) {
        env.remove(QString::fromLatin1(name));
    }
    env.insert(QStringLiteral("LC_ALL"), QStringLiteral("C"));
    env.insert(QStringLiteral("LANG"), QStringLiteral("C"));
    env.remove(QStringLiteral("LANGUAGE"));
    proc->setProcessEnvironment(env);
}

void drainProcessOutput(QProcess *proc) {
    while (proc->bytesAvailable() > 0) {
        proc->read(kStreamChunkBytes);
    }
}

QString versionFromTag(const QString &tag) {
    static const QRegularExpression re(
        QStringLiteral("^v?([0-9]+(?:\\.[0-9]+){2}(?:[-+][0-9A-Za-z.-]+)?)$"));
    const QRegularExpressionMatch match = re.match(tag);
    return match.hasMatch() ? match.captured(1) : QString();
}

QString engineFileName(const QString &version) {
#if defined(Q_OS_WIN)
    return "Bitfox " + version + ".exe";
#else
    return "Bitfox " + version;
#endif
}

QString expectedAssetName(const QString &tag, const QString &suffix) {
    const QString version = versionFromTag(tag);
    if (version.isEmpty() || suffix.isEmpty()) {
        return QString();
    }
    return "bitfox-" + version + suffix;
}

bool validReleaseUrl(const QString &value, const QString &tag, const QString &assetName) {
    const QUrl url(value);
    const QString expectedPath =
        "/Azeel1/bitfox-engine/releases/download/" + tag + "/" + assetName;
    return trustedResponseUrl(url) &&
           url.host().compare(QLatin1String("github.com"), Qt::CaseInsensitive) == 0 &&
           url.path() == expectedPath && !url.hasQuery() && !url.hasFragment();
}

QString archiveMember(const QString &assetName) {
    QString root = assetName;
    if (root.endsWith(QLatin1String(".tar.gz"))) {
        root.chop(7);
    } else if (root.endsWith(QLatin1String(".zip"))) {
        root.chop(4);
    } else {
        return QString();
    }
#if defined(Q_OS_WIN)
    return root + "/bitfox.exe";
#else
    return root + "/bitfox";
#endif
}

void stopProcess(QProcess *proc) {
    if (proc->state() != QProcess::NotRunning) {
        proc->kill();
        if (!proc->waitForFinished(1000)) {
            proc->disconnect();
            proc->setParent(nullptr);
            QObject::connect(proc, &QProcess::readyReadStandardOutput, proc,
                             [proc] { drainProcessOutput(proc); });
            QObject::connect(proc,
                             QOverload<int, QProcess::ExitStatus>::of(&QProcess::finished),
                             proc, &QObject::deleteLater);
            return;
        }
    }
    proc->deleteLater();
}
}

ReleaseManager::ReleaseManager(QObject *parent, const QString &engineDirectory)
    : QObject(parent), m_net(new QNetworkAccessManager(this)),
      m_engineDirectory(engineDirectory.isEmpty() ? enginesDir() : engineDirectory) {}

ReleaseManager::~ReleaseManager() {
    const auto timers = findChildren<QTimer *>();
    for (QTimer *timer : timers) {
        QObject::disconnect(timer, nullptr, this, nullptr);
        timer->stop();
    }
    const auto replies = findChildren<QNetworkReply *>();
    for (QNetworkReply *reply : replies) {
        QObject::disconnect(reply, nullptr, this, nullptr);
        reply->abort();
    }

    const auto processes = findChildren<QProcess *>(QString(), Qt::FindDirectChildrenOnly);
    for (QProcess *proc : processes) {
        QObject::disconnect(proc, nullptr, this, nullptr);
        if (proc->state() != QProcess::NotRunning) {
            proc->kill();
        }
    }
    QElapsedTimer elapsed;
    elapsed.start();
    for (QProcess *proc : processes) {
        const int remaining = qMax(0, 1000 - static_cast<int>(elapsed.elapsed()));
        if (proc->state() != QProcess::NotRunning &&
            (remaining == 0 || !proc->waitForFinished(remaining))) {
            proc->setParent(nullptr);
            connect(proc, &QProcess::readyReadStandardOutput, proc,
                    [proc] { drainProcessOutput(proc); });
            connect(proc, QOverload<int, QProcess::ExitStatus>::of(&QProcess::finished), proc,
                    &QObject::deleteLater);
        }
    }
}

QString ReleaseManager::enginesDir() {
    QString base = QStandardPaths::writableLocation(QStandardPaths::AppLocalDataLocation);
    if (base.isEmpty()) {
        base = QCoreApplication::applicationDirPath();
    }
    return QDir(base).filePath("engines");
}

QString ReleaseManager::platformAssetSuffix() {
    const QString arch = QSysInfo::currentCpuArchitecture().toLower();
#if defined(Q_OS_MACOS)
    if (arch == "arm64" || arch == "aarch64") {
        return "-macos-arm64.tar.gz";
    }
    if (arch == "x86_64" || arch == "amd64") {
        return "-macos-x86_64.tar.gz";
    }
#elif defined(Q_OS_WIN)
    if (arch == "x86_64" || arch == "amd64") {
        return "-windows-x86_64.zip";
    }
#elif defined(Q_OS_LINUX)
    if (arch == "arm64" || arch == "aarch64") {
        return "-linux-aarch64.tar.gz";
    }
    if (arch == "x86_64" || arch == "amd64") {
        return "-linux-x86_64.tar.gz";
    }
#endif
    return QString();
}

bool ReleaseManager::isInstalled(const QString &tag, const QString &engineDirectory) {
    const QString version = versionFromTag(tag);
    if (version.isEmpty()) {
        return false;
    }
    const QString name = engineFileName(version);
    const QStringList roots = {engineDirectory.isEmpty() ? enginesDir() : engineDirectory,
                               QDir(QCoreApplication::applicationDirPath()).filePath("engines")};
    for (const QString &root : roots) {
#if defined(Q_OS_WIN)
        const QFileInfo directory(QDir(root).filePath("Bitfox " + version));
        const QFileInfo packaged(QDir(directory.absoluteFilePath()).filePath(name));
        const QFileInfo inventory(QDir(directory.absoluteFilePath()).filePath("RUNTIME_DEPENDENCIES.json"));
        if (directory.isDir() && !directory.isSymLink() &&
            packaged.isFile() && !packaged.isSymLink() &&
            inventory.isFile() && !inventory.isSymLink()) return true;
#endif
        const QFileInfo info(QDir(root).filePath(name));
        bool installed = info.isFile() && !info.isSymLink();
#if !defined(Q_OS_WIN)
        installed = installed && info.isExecutable();
#endif
        if (installed) {
            return true;
        }
    }
    return false;
}

void ReleaseManager::fetchReleases() {
    const QString suffix = platformAssetSuffix();
    if (suffix.isEmpty()) {
        emit releasesReady({});
        return;
    }

    QNetworkReply *reply = m_net->get(requestFor(kReleasesUrl));
    guardRedirects(reply);
    auto body = std::make_shared<QByteArray>();
    auto oversized = std::make_shared<bool>(false);
    connect(reply, &QNetworkReply::readyRead, this, [reply, body, oversized] {
        if (!*oversized &&
            !appendDeviceData(reply, body.get(), kMaxReleaseResponseBytes)) {
            *oversized = true;
            reply->abort();
        }
    });
    armTotalTimeout(reply, 60000);
    connect(reply, &QNetworkReply::finished, this, [this, reply, body, oversized] {
        reply->deleteLater();
        if (*oversized) {
            emit fetchFailed(tr("GitHub response is too large"));
            return;
        }
        if (reply->error() != QNetworkReply::NoError) {
            emit fetchFailed(networkError(reply));
            return;
        }
        if (!trustedResponseUrl(reply->url()) ||
            !appendDeviceData(reply, body.get(), kMaxReleaseResponseBytes)) {
            emit fetchFailed(tr("unexpected response from GitHub"));
            return;
        }
        const QJsonDocument doc = QJsonDocument::fromJson(*body);
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
            const QString version = versionFromTag(info.tag);
            if (version.isEmpty()) {
                continue;
            }
            const QString expectedAsset = "bitfox-" + version + suffix;
            info.name = obj.value("name").toString();
            if (info.name.isEmpty()) {
                info.name = info.tag;
            }
            int assetCount = 0;
            int checksumCount = 0;
            for (const QJsonValue &av : obj.value("assets").toArray()) {
                const QJsonObject asset = av.toObject();
                const QString assetName = asset.value("name").toString();
                if (assetName == expectedAsset) {
                    ++assetCount;
                    info.assetName = assetName;
                    info.assetUrl = asset.value("browser_download_url").toString();
                } else if (assetName == expectedAsset + QLatin1String(".sha256")) {
                    ++checksumCount;
                    info.checksumUrl = asset.value("browser_download_url").toString();
                }
            }
            if (assetCount != 1 || checksumCount != 1 || info.assetUrl.isEmpty() ||
                info.checksumUrl.isEmpty()) {
                continue;
            }
            info.installed = isInstalled(info.tag, m_engineDirectory);
            releases.append(info);
        }
        emit releasesReady(releases);
    });
}

void ReleaseManager::downloadRelease(const ReleaseInfo &info) {
    const QString assetName = expectedAssetName(info.tag, platformAssetSuffix());
    if (assetName.isEmpty() || info.assetName != assetName ||
        QFileInfo(info.assetName).fileName() != info.assetName ||
        !validReleaseUrl(info.assetUrl, info.tag, assetName) ||
        !validReleaseUrl(info.checksumUrl, info.tag, assetName + ".sha256")) {
        emit downloadFinished(info.tag, false, tr("no build available for this platform"));
        return;
    }
    const QString inFlightKey = versionFromTag(info.tag);
    if (m_inFlight.contains(inFlightKey)) {
        return;
    }
    m_inFlight.insert(inFlightKey);

    QNetworkReply *reply = m_net->get(requestFor(info.checksumUrl));
    guardRedirects(reply);
    auto body = std::make_shared<QByteArray>();
    auto oversized = std::make_shared<bool>(false);
    connect(reply, &QNetworkReply::readyRead, this, [reply, body, oversized] {
        if (!*oversized && !appendDeviceData(reply, body.get(), kMaxChecksumBytes)) {
            *oversized = true;
            reply->abort();
        }
    });
    armTotalTimeout(reply, 30000);
    connect(reply, &QNetworkReply::finished, this, [this, reply, info, body, oversized] {
        reply->deleteLater();
        if (*oversized) {
            fail(info.tag, {}, tr("release checksum is too large"));
            return;
        }
        if (reply->error() != QNetworkReply::NoError) {
            fail(info.tag, {}, networkError(reply));
            return;
        }
        if (!trustedResponseUrl(reply->url()) ||
            !appendDeviceData(reply, body.get(), kMaxChecksumBytes)) {
            fail(info.tag, {}, tr("invalid release checksum response"));
            return;
        }

        static const QRegularExpression re(
            QStringLiteral("^([0-9A-Fa-f]{64})(?:\\s+\\*?(.+))?$"));
        const QRegularExpressionMatch match =
            re.match(QString::fromLatin1(*body).trimmed());
        if (!match.hasMatch() ||
            (!match.captured(2).isEmpty() && match.captured(2) != info.assetName)) {
            fail(info.tag, {}, tr("invalid release checksum"));
            return;
        }
        downloadArchive(info, QByteArray::fromHex(match.captured(1).toLatin1()));
    });
}

void ReleaseManager::downloadArchive(const ReleaseInfo &info, const QByteArray &expectedHash) {
    QString tempRoot = QStandardPaths::writableLocation(QStandardPaths::TempLocation);
    if (tempRoot.isEmpty()) {
        tempRoot = QDir::tempPath();
    }
    auto workDir =
        std::make_shared<QTemporaryDir>(QDir(tempRoot).filePath("bitfox-download-XXXXXX"));
    if (!workDir->isValid()) {
        fail(info.tag, workDir, tr("could not create temporary directory"));
        return;
    }
    const QString archivePath = workDir->filePath(info.assetName);
    auto output = std::make_shared<QFile>(archivePath);
    if (!output->open(QIODevice::WriteOnly | QIODevice::NewOnly)) {
        fail(info.tag, workDir, tr("could not write temporary file"));
        return;
    }

    QNetworkReply *reply = m_net->get(requestFor(info.assetUrl));
    guardRedirects(reply);
    auto written = std::make_shared<qint64>(0);
    auto streamError = std::make_shared<StreamResult>(StreamResult::Ok);
    auto hash = std::make_shared<QCryptographicHash>(QCryptographicHash::Sha256);
    connect(reply, &QNetworkReply::readyRead, this,
            [reply, output, written, streamError, hash] {
                if (*streamError != StreamResult::Ok) {
                    return;
                }
                *streamError =
                    writeDeviceData(reply, output.get(), hash.get(), written.get());
                if (*streamError != StreamResult::Ok) {
                    reply->abort();
                }
            });
    connect(reply, &QNetworkReply::downloadProgress, this,
            [this, reply, tag = info.tag, streamError](qint64 received, qint64 total) {
                if (total > kMaxArchiveBytes && *streamError == StreamResult::Ok) {
                    *streamError = StreamResult::TooLarge;
                    reply->abort();
                }
                emit downloadProgress(tag, received, total);
            });
    armTotalTimeout(reply, 120000);
    connect(reply, &QNetworkReply::finished, this,
            [this, reply, info, expectedHash, archivePath, workDir, output, written,
             streamError, hash] {
                reply->deleteLater();
                if (*streamError == StreamResult::Ok) {
                    *streamError =
                        writeDeviceData(reply, output.get(), hash.get(), written.get());
                }
                if (*streamError == StreamResult::TooLarge) {
                    output->close();
                    fail(info.tag, workDir, tr("downloaded archive is too large"));
                    return;
                }
                if (*streamError == StreamResult::WriteFailed) {
                    output->close();
                    fail(info.tag, workDir, tr("could not write temporary file"));
                    return;
                }
                if (reply->error() != QNetworkReply::NoError) {
                    output->close();
                    fail(info.tag, workDir, networkError(reply));
                    return;
                }
                if (!trustedResponseUrl(reply->url()) || *written <= 0 || !output->flush()) {
                    output->close();
                    fail(info.tag, workDir, tr("downloaded archive has an invalid response"));
                    return;
                }
                output->close();
                if (output->error() != QFileDevice::NoError) {
                    fail(info.tag, workDir, tr("could not write temporary file"));
                    return;
                }
                if (hash->result() != expectedHash) {
                    fail(info.tag, workDir,
                         tr("downloaded archive failed checksum verification"));
                    return;
                }
                handleDownloadedArchive(info.tag, archivePath, workDir);
            });
}

void ReleaseManager::fail(const QString &tag, const std::shared_ptr<QTemporaryDir> &,
                          const QString &error) {
    m_inFlight.remove(versionFromTag(tag));
    emit downloadFinished(tag, false, error);
}

void ReleaseManager::handleDownloadedArchive(
    const QString &tag, const QString &archivePath,
    const std::shared_ptr<QTemporaryDir> &workDir) {
    const QString member = archiveMember(QFileInfo(archivePath).fileName());
    if (member.isEmpty()) {
        fail(tag, workDir, tr("unsupported release archive"));
        return;
    }
    const QString tool = archiveTool();
    if (tool.isEmpty()) {
        fail(tag, workDir, tr("could not find the system archive extractor"));
        return;
    }

    auto *proc = new QProcess(this);
    proc->setStandardErrorFile(QProcess::nullDevice());
    configureArchiveProcess(proc);
    auto listing = std::make_shared<QByteArray>();
    listing->reserve(4096);
#if defined(Q_OS_WIN)
    const QStringList arguments = {"-tvf", archivePath};
#else
    const QStringList arguments = {"-tvzf", archivePath, member};
#endif
    auto settled = std::make_shared<bool>(false);
    connect(proc, &QProcess::readyReadStandardOutput, this,
            [this, proc, tag, workDir, listing, settled] {
                if (!*settled &&
                    !appendDeviceData(proc, listing.get(), kMaxArchiveListingBytes)) {
                    *settled = true;
                    stopProcess(proc);
                    fail(tag, workDir, tr("release archive contains too many entries"));
                }
            });
    connect(proc, &QProcess::errorOccurred, this,
            [this, proc, tag, workDir, listing, settled](QProcess::ProcessError err) {
                if (*settled || err != QProcess::FailedToStart) {
                    return;
                }
                *settled = true;
                stopProcess(proc);
                fail(tag, workDir, tr("could not run archive extractor"));
            });
    connect(proc, QOverload<int, QProcess::ExitStatus>::of(&QProcess::finished), this,
            [this, proc, tag, archivePath, member, workDir, listing, settled](
                int exitCode, QProcess::ExitStatus status) {
                if (*settled) {
                    proc->deleteLater();
                    return;
                }
                *settled = true;
                proc->deleteLater();
                if (status != QProcess::NormalExit || exitCode != 0) {
                    fail(tag, workDir, tr("could not extract downloaded archive"));
                    return;
                }
                if (!appendDeviceData(proc, listing.get(), kMaxArchiveListingBytes)) {
                    fail(tag, workDir, tr("release archive contains too many entries"));
                    return;
                }
                int matches = 0;
                const QByteArray expected = member.toUtf8();
                for (QByteArray line : listing->split('\n')) {
                    if (line.endsWith('\r')) {
                        line.chop(1);
                    }
                    if (line.endsWith(QByteArray(" ") + expected)) {
                        if (!line.startsWith('-')) {
                            fail(tag, workDir,
                                 tr("release archive member is not a regular file"));
                            return;
                        }
                        ++matches;
                    }
                }
                if (matches != 1) {
                    fail(tag, workDir, tr("release archive has an invalid layout"));
                    return;
                }
#if defined(Q_OS_WIN)
                extractWindowsPackage(tag, archivePath, member, *listing, workDir);
#else
                extractArchiveMember(tag, archivePath, member, workDir);
#endif
            });
    auto *timeout = new QTimer(proc);
    timeout->setSingleShot(true);
    connect(proc, QOverload<int, QProcess::ExitStatus>::of(&QProcess::finished), timeout,
            &QTimer::stop);
    connect(timeout, &QTimer::timeout, this, [this, proc, tag, workDir, settled] {
        if (*settled) {
            return;
        }
        *settled = true;
        stopProcess(proc);
        fail(tag, workDir, tr("archive inspection timed out"));
    });
    timeout->start(30000);
    proc->start(tool, arguments);
}

void ReleaseManager::extractArchiveMember(
    const QString &tag, const QString &archivePath, const QString &member,
    const std::shared_ptr<QTemporaryDir> &workDir) {
    extractMemberToFile(tag, archivePath, member, QStringLiteral("bitfox-engine"),
                        kMaxArchiveBytes, workDir,
                        [this, tag, workDir](const QString &path) {
                            installExtractedBinary(tag, path, workDir);
                        });
}

void ReleaseManager::extractMemberToFile(
    const QString &tag, const QString &archivePath, const QString &member,
    const QString &targetName, qint64 limit,
    const std::shared_ptr<QTemporaryDir> &workDir,
    std::function<void(const QString &)> completed) {
    const QString binaryPath = workDir->filePath(targetName);
    const QString tool = archiveTool();
    if (tool.isEmpty()) {
        fail(tag, workDir, tr("could not find the system archive extractor"));
        return;
    }
    auto output = std::make_shared<QFile>(binaryPath);
    if (!output->open(QIODevice::WriteOnly | QIODevice::NewOnly)) {
        fail(tag, workDir, tr("could not create temporary engine file"));
        return;
    }
    auto *proc = new QProcess(this);
    proc->setStandardErrorFile(QProcess::nullDevice());
    configureArchiveProcess(proc);
#if defined(Q_OS_WIN)
    const QStringList arguments = {"-xOf", archivePath, member};
#else
    const QStringList arguments = {"-xOzf", archivePath, member};
#endif
    auto settled = std::make_shared<bool>(false);
    auto written = std::make_shared<qint64>(0);
    auto streamError = std::make_shared<StreamResult>(StreamResult::Ok);
    connect(proc, &QProcess::readyReadStandardOutput, this,
            [this, proc, tag, workDir, output, written, streamError, settled, limit] {
                if (*settled || *streamError != StreamResult::Ok) {
                    return;
                }
                *streamError =
                    writeDeviceData(proc, output.get(), nullptr, written.get(), limit);
                if (*streamError == StreamResult::Ok) {
                    return;
                }
                *settled = true;
                output->close();
                stopProcess(proc);
                fail(tag, workDir,
                     *streamError == StreamResult::TooLarge
                         ? tr("archive engine binary is too large")
                         : tr("could not write temporary engine file"));
            });
    connect(proc, &QProcess::errorOccurred, this,
            [this, proc, tag, workDir, output, settled](QProcess::ProcessError err) {
                if (*settled || err != QProcess::FailedToStart) {
                    return;
                }
                *settled = true;
                output->close();
                stopProcess(proc);
                fail(tag, workDir, tr("could not run archive extractor"));
            });
    connect(proc, QOverload<int, QProcess::ExitStatus>::of(&QProcess::finished), this,
            [this, proc, tag, binaryPath, workDir, output, written, streamError,
             settled, limit, completed](int exitCode, QProcess::ExitStatus status) {
                if (*settled) {
                    proc->deleteLater();
                    return;
                }
                *streamError =
                    writeDeviceData(proc, output.get(), nullptr, written.get(), limit);
                *settled = true;
                proc->deleteLater();
                const bool flushed = output->flush();
                output->close();
                const QFileInfo binary(binaryPath);
                if (*streamError != StreamResult::Ok || !flushed ||
                    output->error() != QFileDevice::NoError ||
                    status != QProcess::NormalExit || exitCode != 0 || *written <= 0 ||
                    !binary.isFile() || binary.isSymLink() || binary.size() != *written) {
                    fail(tag, workDir, tr("archive contains an invalid engine binary"));
                    return;
                }
                completed(binaryPath);
            });
    auto *timeout = new QTimer(proc);
    timeout->setSingleShot(true);
    connect(proc, QOverload<int, QProcess::ExitStatus>::of(&QProcess::finished), timeout,
            &QTimer::stop);
    connect(timeout, &QTimer::timeout, this,
            [this, proc, tag, workDir, output, settled] {
                if (*settled) {
                    return;
                }
                *settled = true;
                output->close();
                stopProcess(proc);
                fail(tag, workDir, tr("archive extraction timed out"));
            });
    timeout->start(30000);
    proc->start(tool, arguments);
}

#if defined(Q_OS_WIN)
namespace {
const QSet<QString> kRuntimeDlls = {
    "msvcp140.dll", "msvcp140_1.dll", "msvcp140_2.dll",
    "msvcp140_atomic_wait.dll", "msvcp140_codecvt_ids.dll",
    "vcruntime140.dll", "vcruntime140_1.dll", "concrt140.dll"
};

bool regularArchiveMember(const QByteArray &listing, const QString &member) {
    int count = 0;
    bool regular = true;
    const QString suffix = QLatin1Char(' ') + member;

    for (QByteArray line : listing.split('\n')) {
        if (line.endsWith('\r')) {
            line.chop(1);
        }
        QString text = QString::fromUtf8(line);
        if (line.startsWith('l')) {
            const qsizetype separator = text.indexOf(" -> ");
            if (separator >= 0) {
                text.truncate(separator);
            }
        } else if (line.startsWith('h')) {
            const qsizetype separator = text.indexOf(" link to ");
            if (separator >= 0) {
                text.truncate(separator);
            }
        }
        if (text.endsWith(suffix, Qt::CaseInsensitive)) {
            ++count;
            regular = regular && line.startsWith('-') && text.endsWith(suffix);
        }
    }
    return count == 1 && regular;
}

bool runtimeInventory(QJsonObject *inventory, bool archive,
                      QVector<QPair<QString, QByteArray>> *members) {
    static const QRegularExpression hash(QStringLiteral("^[0-9a-f]{64}$"));
    static const QRegularExpression version(QStringLiteral("^[0-9]+(?:[., ][0-9]+)*$"));

    if (inventory->value("schema").toInt() != 1 ||
        inventory->value("platform").toString() != "windows" ||
        inventory->value("architecture").toString() != "x86_64" ||
        !inventory->value("origin").isObject() ||
        !inventory->value("executables").isArray() ||
        !inventory->value("dependencies").isArray() ||
        !inventory->value("notices").isArray()) {
        return false;
    }

    const QJsonArray executables = inventory->value("executables").toArray();
    const QJsonValue expectedExe = archive ? QJsonValue("bitfox.exe")
                                           : QJsonValue("bitfox-board.exe");
    if (!executables.contains(expectedExe)) {
        return false;
    }

    const QJsonObject origin = inventory->value("origin").toObject();
    if (origin.value("provider").toString() != "Visual Studio 2022" ||
        origin.value("directory").toString() != "x64/Microsoft.VC143.CRT" ||
        !version.match(origin.value("redist_version").toString()).hasMatch()) {
        return false;
    }

    const QJsonArray dependencies = inventory->value("dependencies").toArray();
    if (dependencies.size() < 3 || dependencies.size() > 16) {
        return false;
    }

    QSet<QString> seen;
    QSet<QString> rootNames;
    QJsonArray selected;
    for (const QJsonValue &value : dependencies) {
        if (!value.isObject()) {
            return false;
        }
        const QJsonObject entry = value.toObject();
        const QString path = entry.value("path").toString();
        const QString name = path.startsWith("engines/") ? path.mid(8) : path;
        if (!kRuntimeDlls.contains(name) || seen.contains(path) ||
            (path != name && (archive || path != "engines/" + name)) ||
            entry.value("provider").toString() != "msvc" ||
            !version.match(entry.value("version").toString()).hasMatch() ||
            !entry.value("copyright").toString().contains("Microsoft") ||
            !hash.match(entry.value("sha256").toString()).hasMatch()) {
            return false;
        }
        seen.insert(path);
        if (path == name) {
            rootNames.insert(name);
            selected.append(entry);
            members->append({name, QByteArray::fromHex(entry.value("sha256").toString().toLatin1())});
        }
    }

    if (!rootNames.contains("msvcp140.dll") ||
        !rootNames.contains("vcruntime140.dll") ||
        !rootNames.contains("vcruntime140_1.dll")) {
        return false;
    }

    const QJsonArray notices = inventory->value("notices").toArray();
    if (notices.size() != 1 || !notices.first().isObject()) {
        return false;
    }

    const QJsonObject notice = notices.first().toObject();
    if (notice.value("path").toString() != "licenses/msvc-2022.txt" ||
        notice.value("provider").toString() != "msvc" ||
        notice.value("version").toString().isEmpty() ||
        !notice.value("source").toString().startsWith("https://") ||
        !hash.match(notice.value("sha256").toString()).hasMatch()) {
        return false;
    }

    members->append({notice.value("path").toString(),
                     QByteArray::fromHex(notice.value("sha256").toString().toLatin1())});
    inventory->insert("dependencies", selected);
    return true;
}

QByteArray runtimeFileHash(const QString &path) {
    const QFileInfo info(path);
    if (!info.isFile() || info.isSymLink() || info.size() <= 0 ||
        info.size() > 16 * 1024 * 1024) {
        return {};
    }
    QFile file(path);
    QCryptographicHash hash(QCryptographicHash::Sha256);
    if (!file.open(QIODevice::ReadOnly) || !hash.addData(&file) ||
        file.error() != QFileDevice::NoError) {
        return {};
    }
    return hash.result();
}

bool matchesRuntimeHash(const QString &path, const QByteArray &expected) {
    return expected.size() == 32 && runtimeFileHash(path) == expected;
}
}

struct ReleaseManager::WindowsInstallation {
    QString tag;
    QString archive;
    QString root;
    QString binary;
    QByteArray listing;
    QJsonObject inventory;
    QVector<QPair<QString, QByteArray>> members;
    QMap<QString, QString> files;
    std::shared_ptr<QTemporaryDir> work;
    qsizetype index = 0;
};

void ReleaseManager::extractWindowsPackage(
    const QString &tag, const QString &archivePath, const QString &member,
    const QByteArray &listing, const std::shared_ptr<QTemporaryDir> &workDir) {
    if (!regularArchiveMember(listing, member)) {
        fail(tag, workDir, tr("release archive has an invalid layout"));
        return;
    }
    const QString root = member.left(member.lastIndexOf('/'));
    const QString manifestMember = root + "/RUNTIME_DEPENDENCIES.json";
    if (!listing.toLower().contains(manifestMember.toUtf8().toLower())) {
        extractArchiveMember(tag, archivePath, member, workDir);
        return;
    }
    if (!regularArchiveMember(listing, manifestMember)) {
        fail(tag, workDir, tr("release runtime inventory is not a regular file"));
        return;
    }
    auto installation = std::make_shared<WindowsInstallation>();
    installation->tag = tag;
    installation->archive = archivePath;
    installation->root = root;
    installation->listing = listing;
    installation->work = workDir;
    extractMemberToFile(tag, archivePath, member, "bitfox-engine", kMaxArchiveBytes,
                        workDir, [this, installation, manifestMember](const QString &binary) {
        installation->binary = binary;
        extractMemberToFile(installation->tag, installation->archive, manifestMember,
                            "runtime-manifest", 64 * 1024, installation->work,
                            [this, installation](const QString &path) {
            QFile file(path);
            if (!file.open(QIODevice::ReadOnly)) {
                fail(installation->tag, installation->work, tr("could not read runtime inventory"));
                return;
            }
            const QJsonDocument document = QJsonDocument::fromJson(file.readAll());
            installation->inventory = document.object();
            if (!document.isObject() ||
                !runtimeInventory(&installation->inventory, true, &installation->members)) {
                fail(installation->tag, installation->work, tr("invalid release runtime inventory"));
                return;
            }
            installation->members.append({QStringLiteral("LICENSE"), QByteArray()});
            installation->members.append({QStringLiteral("THIRD_PARTY_NOTICES.txt"), QByteArray()});
            extractWindowsDependency(installation);
        });
    });
}

void ReleaseManager::extractWindowsDependency(
    const std::shared_ptr<WindowsInstallation> &installation) {
    if (installation->index == installation->members.size()) {
        installWindowsDirectory(installation);
        return;
    }
    const auto entry = installation->members.at(installation->index);
    const QString member = installation->root + "/" + entry.first;
    if (!regularArchiveMember(installation->listing, member)) {
        fail(installation->tag, installation->work, tr("release package is missing a runtime file"));
        return;
    }
    const QString target = "runtime-part-" + QString::number(installation->index);
    extractMemberToFile(installation->tag, installation->archive, member, target,
                        16 * 1024 * 1024, installation->work,
                        [this, installation, entry](const QString &path) {
        const QByteArray actual = runtimeFileHash(path);
        if (actual.size() != 32 || (!entry.second.isEmpty() && actual != entry.second)) {
            fail(installation->tag, installation->work, tr("runtime file failed checksum verification"));
            return;
        }
        installation->members[installation->index].second = actual;
        installation->files.insert(entry.first, path);
        ++installation->index;
        extractWindowsDependency(installation);
    });
}

void ReleaseManager::installWindowsDirectory(
    const std::shared_ptr<WindowsInstallation> &installation) {
    const QString version = versionFromTag(installation->tag);
    if (version.isEmpty() || !QDir().mkpath(m_engineDirectory)) {
        fail(installation->tag, installation->work, tr("could not create engine directory"));
        return;
    }
    const QString destination = QDir(m_engineDirectory).filePath("Bitfox " + version);
    const QFileInfo existing(destination);
    if (existing.exists() || existing.isSymLink()) {
        fail(installation->tag, installation->work, tr("engine destination already exists"));
        return;
    }
    QTemporaryDir stage(QDir(m_engineDirectory).filePath("install-XXXXXX"));
    const QString binaryName = engineFileName(version);
    if (!stage.isValid() || !QFile::copy(installation->binary, stage.filePath(binaryName)) ||
        !QDir(stage.path()).mkpath("licenses")) {
        fail(installation->tag, installation->work, tr("could not stage engine package"));
        return;
    }
    for (const auto &entry : installation->members) {
        const QString source = installation->files.value(entry.first);
        const QString target = stage.filePath(entry.first);
        if (source.isEmpty() || !matchesRuntimeHash(source, entry.second) ||
            !QFile::copy(source, target) || !matchesRuntimeHash(target, entry.second)) {
            fail(installation->tag, installation->work, tr("could not stage verified runtime files"));
            return;
        }
    }
    QJsonObject inventory = installation->inventory;
    inventory.insert("executables", QJsonArray{binaryName});
    QFile manifest(stage.filePath("RUNTIME_DEPENDENCIES.json"));
    const QByteArray encoded = QJsonDocument(inventory).toJson();
    if (!manifest.open(QIODevice::WriteOnly | QIODevice::NewOnly) ||
        manifest.write(encoded) != encoded.size() || !manifest.flush()) {
        fail(installation->tag, installation->work, tr("could not save runtime inventory"));
        return;
    }
    manifest.close();
    if (manifest.error() != QFileDevice::NoError) {
        fail(installation->tag, installation->work, tr("could not save runtime inventory"));
        return;
    }
    if (!QDir().rename(stage.path(), destination)) {
        fail(installation->tag, installation->work, tr("could not install engine package"));
        return;
    }
    stage.setAutoRemove(false);
    m_inFlight.remove(versionFromTag(installation->tag));
    emit downloadFinished(installation->tag, true, QString());
}

bool ReleaseManager::installWindowsFallback(
    const QString &tag, const QString &binaryPath,
    const std::shared_ptr<QTemporaryDir> &workDir) {
    const QString root = QCoreApplication::applicationDirPath();
    QFile file(QDir(root).filePath("RUNTIME_DEPENDENCIES.json"));
    const QFileInfo info(file);
    if (!info.exists() && !info.isSymLink()) return false;
    auto installation = std::make_shared<WindowsInstallation>();
    installation->tag = tag;
    installation->binary = binaryPath;
    installation->work = workDir;
    if (info.isSymLink() || info.size() > 64 * 1024 || !file.open(QIODevice::ReadOnly)) {
        fail(tag, workDir, tr("could not read bundled runtime inventory"));
        return true;
    }
    const QJsonDocument document = QJsonDocument::fromJson(file.readAll());
    installation->inventory = document.object();
    if (!document.isObject() ||
        !runtimeInventory(&installation->inventory, false, &installation->members)) {
        fail(tag, workDir, tr("invalid bundled runtime inventory"));
        return true;
    }
    const QString canonicalRoot = QDir(root).canonicalPath() + "/";
    for (const auto &entry : installation->members) {
        const QString path = QDir(root).filePath(entry.first);
        if (!QFileInfo(path).canonicalFilePath().startsWith(canonicalRoot, Qt::CaseInsensitive) ||
            !matchesRuntimeHash(path, entry.second)) {
            fail(tag, workDir, tr("bundled runtime file failed verification"));
            return true;
        }
        installation->files.insert(entry.first, path);
    }
    installWindowsDirectory(installation);
    return true;
}
#endif

void ReleaseManager::installExtractedBinary(
    const QString &tag, const QString &binaryPath,
    const std::shared_ptr<QTemporaryDir> &workDir) {

#if defined(Q_OS_WIN)
    if (installWindowsFallback(tag, binaryPath, workDir)) {
        return;
    }
#endif
    const QString version = versionFromTag(tag);
    if (version.isEmpty() || !QDir().mkpath(m_engineDirectory)) {
        fail(tag, workDir, tr("could not create engine directory"));
        return;
    }
    const QString dest = QDir(m_engineDirectory).filePath(engineFileName(version));
    QFile source(binaryPath);
    QSaveFile output(dest);
    output.setDirectWriteFallback(false);
    if (!source.open(QIODevice::ReadOnly) || !output.open(QIODevice::WriteOnly)) {
        fail(tag, workDir, tr("could not install engine binary"));
        return;
    }
    while (!source.atEnd()) {
        const QByteArray chunk = source.read(1024 * 1024);
        if (chunk.isEmpty() || output.write(chunk) != chunk.size()) {
            output.cancelWriting();
            fail(tag, workDir, tr("could not install engine binary"));
            return;
        }
    }
    if (source.error() != QFileDevice::NoError) {
        output.cancelWriting();
        fail(tag, workDir, tr("could not install engine binary"));
        return;
    }
#if !defined(Q_OS_WIN)
    const QFileDevice::Permissions permissions =
        QFileDevice::ReadOwner | QFileDevice::WriteOwner | QFileDevice::ExeOwner |
        QFileDevice::ReadGroup | QFileDevice::ExeGroup | QFileDevice::ReadOther |
        QFileDevice::ExeOther;
    if (!output.setPermissions(permissions)) {
        output.cancelWriting();
        fail(tag, workDir, tr("could not install engine binary"));
        return;
    }
#endif
    if (!output.commit()) {
        fail(tag, workDir, tr("could not install engine binary"));
        return;
    }

    m_inFlight.remove(versionFromTag(tag));
    emit downloadFinished(tag, true, QString());
}
