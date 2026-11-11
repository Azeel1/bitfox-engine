#include "update/AppUpdater.h"

#include <QCoreApplication>
#include <QCryptographicHash>
#include <QDateTime>
#include <QDir>
#include <QElapsedTimer>
#include <QFile>
#include <QFileInfo>
#include <QJsonArray>
#include <QJsonDocument>
#include <QJsonObject>
#include <QLockFile>
#include <QNetworkAccessManager>
#include <QNetworkReply>
#include <QNetworkRequest>
#include <QRegularExpression>
#include <QSaveFile>
#include <QSettings>
#include <QStandardPaths>
#include <QSysInfo>
#include <QTemporaryDir>
#include <QTimer>
#include <QUrl>
#include <cmath>
#include <memory>

namespace {
constexpr qint64 kMaxPackageBytes = 512LL * 1024 * 1024;
const QString kLatest = QStringLiteral("https://api.github.com/repos/Azeel1/bitfox-engine/releases/latest");

struct Version {
    QStringList core;
    QStringList pre;
    bool valid = false;
};

Version version(QString value) {
    static const QRegularExpression pattern(QStringLiteral(
        "^v?(0|[1-9][0-9]*)\\.(0|[1-9][0-9]*)\\.(0|[1-9][0-9]*)"
        "(?:-([0-9A-Za-z-]+(?:\\.[0-9A-Za-z-]+)*))?"
        "(?:\\+([0-9A-Za-z-]+(?:\\.[0-9A-Za-z-]+)*))?$"));
    static const QRegularExpression digits(QStringLiteral("^[0-9]+$"));
    const auto match = pattern.match(value);
    Version result;
    if (!match.hasMatch()) return result;
    result.core = {match.captured(1), match.captured(2), match.captured(3)};
    if (!match.captured(4).isEmpty()) {
        result.pre = match.captured(4).split('.');
        for (const QString &part : result.pre) {
            if (part.size() > 1 && part.startsWith('0') && digits.match(part).hasMatch())
                return result;
        }
    }
    result.valid = true;
    return result;
}

int numericCompare(const QString &a, const QString &b) {
    if (a.size() != b.size()) return a.size() < b.size() ? -1 : 1;
    const int result = QString::compare(a, b, Qt::CaseSensitive);
    return result == 0 ? 0 : result < 0 ? -1 : 1;
}

bool officialAsset(const QString &value, const QString &tag, const QString &name) {
    const QUrl url(value);
    return AppUpdater::trustedUrl(url) && url.host() == QStringLiteral("github.com") &&
           url.path() == QStringLiteral("/Azeel1/bitfox-engine/releases/download/") + tag + '/' + name &&
           !url.hasQuery() && !url.hasFragment();
}

struct PackageCacheIO {
    PackageCacheIO(const QString &source, const QString &destination, const QString &directory,
                   std::function<void(bool)> completion)
        : input(source), output(destination), ownedDirectory(directory), complete(std::move(completion)) {}
    ~PackageCacheIO() {
        input.close();
        output.close();
        if (!retained) {
            if (outputCreated) output.remove();
            if (!ownedDirectory.isEmpty()) QDir().rmdir(ownedDirectory);
        }
    }
    QFile input;
    QFile output;
    QString ownedDirectory;
    std::function<void(bool)> complete;
    bool outputCreated = false;
    bool retained = false;
};

bool readBounded(QNetworkReply *reply, QByteArray *body, qsizetype limit) {
    while (reply->bytesAvailable() > 0) {
        QByteArray chunk = reply->read(qMin<qint64>(65536, limit - body->size() + 1));
        if (chunk.isEmpty() || chunk.size() > limit - body->size()) return false;
        body->append(chunk);
    }
    return true;
}
}

AppUpdater::AppUpdater(QObject *parent, QNetworkAccessManager *network)
    : QObject(parent), m_network(network ? network : new QNetworkAccessManager(this)) {}

AppUpdater::~AppUpdater() {
    if (m_reply) {
        disconnect(m_reply, nullptr, this, nullptr);
        m_reply->abort();
        m_reply->deleteLater();
    }
}

int AppUpdater::compareVersions(const QString &left, const QString &right, bool *valid) {
    const Version a = version(left);
    const Version b = version(right);
    if (valid) {
        *valid = a.valid && b.valid;
    }
    if (!a.valid || !b.valid) {
        return 0;
    }
    for (int i = 0; i < 3; ++i) {
        const int result = numericCompare(a.core[i], b.core[i]);
        if (result != 0) {
            return result;
        }
    }
    if (a.pre.isEmpty() || b.pre.isEmpty()) {
        if (a.pre.isEmpty() == b.pre.isEmpty()) {
            return 0;
        }
        return a.pre.isEmpty() ? 1 : -1;
    }

    static const QRegularExpression digits(QStringLiteral("^[0-9]+$"));
    for (qsizetype i = 0; i < qMin(a.pre.size(), b.pre.size()); ++i) {
        const bool an = digits.match(a.pre[i]).hasMatch();
        const bool bn = digits.match(b.pre[i]).hasMatch();
        const int result = (an && bn) ? numericCompare(a.pre[i], b.pre[i])
                         : (an != bn) ? (an ? -1 : 1)
                                      : QString::compare(a.pre[i], b.pre[i]);
        if (result != 0) {
            return result < 0 ? -1 : 1;
        }
    }
    return a.pre.size() == b.pre.size() ? 0 : (a.pre.size() < b.pre.size() ? -1 : 1);
}

QString AppUpdater::platformSuffix() {
    const QString arch = QSysInfo::buildCpuArchitecture().toLower();
#if defined(Q_OS_MACOS)
    if (arch == "arm64" || arch == "aarch64") {
        return "-macos-arm64-gui.zip";
    }
    if (arch == "x86_64" || arch == "amd64") {
        return "-macos-x86_64-gui.zip";
    }
#elif defined(Q_OS_WIN)
    if (arch == "x86_64" || arch == "amd64") {
        return "-windows-x86_64-gui.zip";
    }
#elif defined(Q_OS_LINUX)
    if (arch == "x86_64" || arch == "amd64") {
        return "-linux-x86_64-gui.tar.gz";
    }
    if (arch == "arm64" || arch == "aarch64") {
        return "-linux-aarch64-gui.tar.gz";
    }
#endif
    return {};
}

bool AppUpdater::trustedUrl(const QUrl &url) {
    const QString host = url.host().toLower();
    return url.isValid() && url.scheme() == "https" && url.userInfo().isEmpty() &&
           (url.port() == -1 || url.port() == 443) &&
           (host == "api.github.com" || host == "github.com" ||
            host == "release-assets.githubusercontent.com" || host == "objects.githubusercontent.com" ||
            host == "github-releases.githubusercontent.com");
}

bool AppUpdater::parseRelease(const QJsonObject &object, const QString &suffix,
                              AppRelease *release, QString *error) {
    const auto fail = [error](const QString &message) {
        if (error) {
            *error = message;
        }
        return false;
    };

    AppRelease out;
    out.tag = object.value("tag_name").toString();
    const Version parsed = version(out.tag);
    if (!parsed.valid || !parsed.pre.isEmpty() || object.value("draft").toBool(true) ||
        object.value("prerelease").toBool(true)) {
        return fail(tr("The latest release is not a valid stable version."));
    }
    if (suffix.isEmpty()) {
        return fail(tr("No application package supports this platform."));
    }

    out.version = out.tag.startsWith('v') ? out.tag.mid(1) : out.tag;
    out.notes = object.value("body").toString();
    out.assetName = "bitfox-" + out.version + suffix;

    QJsonObject package;
    QJsonObject sums;
    QJsonObject companion;
    int packages = 0;
    int sumsCount = 0;
    int companions = 0;

    for (const QJsonValue &value : object.value("assets").toArray()) {
        const QJsonObject asset = value.toObject();
        const QString name = asset.value("name").toString();
        if (name == out.assetName) {
            package = asset;
            ++packages;
        }
        if (name == "SHA256SUMS") {
            sums = asset;
            ++sumsCount;
        }
        if (name == out.assetName + ".sha256") {
            companion = asset;
            ++companions;
        }
    }

    if (packages == 0) {
        return fail(tr("The latest release has no application package for this platform."));
    }
    if (packages != 1) {
        return fail(tr("Release metadata contains duplicate application packages."));
    }

    const double size = package.value("size").toDouble(-1);
    if (!std::isfinite(size) || size <= 0 || size > kMaxPackageBytes || std::floor(size) != size) {
        return fail(tr("The application package size is invalid."));
    }
    out.size = static_cast<qint64>(size);
    out.assetUrl = package.value("browser_download_url").toString();
    if (!officialAsset(out.assetUrl, out.tag, out.assetName)) {
        return fail(tr("The application package URL is not an official release asset."));
    }

    const QString digest = package.value("digest").toString();
    static const QRegularExpression hash(QStringLiteral("^sha256:([a-fA-F0-9]{64})$"));
    if (!digest.isEmpty()) {
        const auto match = hash.match(digest);
        if (!match.hasMatch()) {
            return fail(tr("The application package digest is invalid."));
        }
        out.digest = QByteArray::fromHex(match.captured(1).toLatin1());
    } else {
        if (sumsCount > 1 || companions > 1) {
            return fail(tr("Release checksums are ambiguous."));
        }
        const QJsonObject checksum = sumsCount == 1 ? sums : companion;
        const QString name = sumsCount == 1 ? QStringLiteral("SHA256SUMS") : out.assetName + ".sha256";
        out.checksumUrl = checksum.value("browser_download_url").toString();
        if (!officialAsset(out.checksumUrl, out.tag, name)) {
            return fail(tr("The release does not provide a trusted SHA-256 checksum."));
        }
    }

    *release = out;
    return true;
}

QByteArray AppUpdater::parseChecksum(const QByteArray &body, const QString &assetName) {
    static const QRegularExpression pattern(QStringLiteral("^([a-fA-F0-9]{64})[ \\t]+\\*?([^\\r\\n]+)$"));
    QByteArray result;
    for (QString line : QString::fromUtf8(body).split('\n')) {
        if (line.endsWith('\r')) {
            line.chop(1);
        }
        if (line.isEmpty()) {
            continue;
        }
        const auto match = pattern.match(line);
        if (!match.hasMatch()) {
            return {};
        }
        if (match.captured(2) == assetName) {
            if (!result.isEmpty()) {
                return {};
            }
            result = QByteArray::fromHex(match.captured(1).toLatin1());
        }
    }
    return result;
}

void AppUpdater::setState(State state, const QString &message) {
    m_state = state;
    m_message = message;
    emit stateChanged();
}

QNetworkReply *AppUpdater::request(const QString &url, int timeoutMs) {
    QNetworkRequest request{QUrl(url)};
    request.setRawHeader("User-Agent", "Bitfox-Board-Updater");
    request.setRawHeader("Accept", "application/vnd.github+json");
    request.setTransferTimeout(30000);
    request.setMaximumRedirectsAllowed(5);
    request.setAttribute(QNetworkRequest::RedirectPolicyAttribute, QNetworkRequest::UserVerifiedRedirectPolicy);
    QNetworkReply *reply = m_network->get(request);
    m_reply = reply;
    reply->setReadBufferSize(128 * 1024);
    connect(reply, &QNetworkReply::redirected, this, [reply](const QUrl &url) {
        if (trustedUrl(url)) reply->redirectAllowed();
        else {
            reply->setProperty("updateError", tr("The download redirected to an untrusted source."));
            reply->abort();
        }
    });
    auto *timer = new QTimer(reply);
    timer->setSingleShot(true);
    connect(timer, &QTimer::timeout, reply, [reply] {
        reply->setProperty("updateError", tr("The network request timed out."));
        reply->abort();
    });
    connect(reply, &QNetworkReply::finished, timer, &QTimer::stop);
    timer->start(timeoutMs);
    return reply;
}

QString AppUpdater::replyError(QNetworkReply *reply) const {
    if (!reply->property("updateError").toString().isEmpty()) return reply->property("updateError").toString();
    if (reply->error() != QNetworkReply::NoError) return reply->errorString();
    if (reply->attribute(QNetworkRequest::HttpStatusCodeAttribute).toInt() != 200 || !trustedUrl(reply->url()))
        return tr("The server returned an invalid response.");
    return {};
}

void AppUpdater::getText(const QString &url, qsizetype limit,
                         std::function<void(const QByteArray &)> complete) {
    QNetworkReply *reply = request(url, 60000);
    auto body = std::make_shared<QByteArray>();
    auto consume = [reply, body, limit] {
        if (!readBounded(reply, body.get(), limit)) {
            reply->setProperty("updateError", tr("The server response is too large."));
            reply->abort();
        }
    };
    connect(reply, &QNetworkReply::readyRead, this, consume);
    connect(reply, &QNetworkReply::finished, this, [this, reply, body, limit, complete] {
        reply->deleteLater();
        if (m_reply != reply) return;
        m_reply = nullptr;
        if (m_state == Cancelled) return;
        const int code = reply->attribute(QNetworkRequest::HttpStatusCodeAttribute).toInt();
        if (code == 403 || code == 429) {
            setState(RateLimited, tr("GitHub has limited update requests. Try again later."));
            return;
        }
        const QString error = replyError(reply);
        if (!error.isEmpty()) {
            const auto e = reply->error();
            setState(e == QNetworkReply::HostNotFoundError || e == QNetworkReply::NetworkSessionFailedError ||
                     e == QNetworkReply::TemporaryNetworkFailureError || e == QNetworkReply::ConnectionRefusedError
                         ? Offline : Failed, error);
            return;
        }
        if (!readBounded(reply, body.get(), limit)) {
            setState(Failed, tr("The server response is too large."));
            return;
        }
        complete(*body);
    });
}

void AppUpdater::check(bool manual) {
    if (busy()) return;
    QSettings settings;
    const qint64 now = QDateTime::currentSecsSinceEpoch();
    const qint64 last = settings.value("appUpdates/lastCheck", 0).toLongLong();
    if (!manual && last <= now && now - last < 24 * 60 * 60) return;
    if (platformSuffix().isEmpty()) {
        setState(Unsupported, tr("Application updates are not available for this platform."));
        return;
    }
    settings.setValue("appUpdates/lastCheck", now);
    m_release = {};
    setState(Checking, tr("Checking for application updates..."));
    getText(kLatest, 4 * 1024 * 1024, [this, manual](const QByteArray &body) {
        const QJsonDocument document = QJsonDocument::fromJson(body);
        if (!document.isObject()) {
            setState(Failed, tr("GitHub returned invalid release metadata."));
            return;
        }
        const auto object = document.object();
        bool valid = false;
        const int comparison = compareVersions(object.value("tag_name").toString(), QCoreApplication::applicationVersion(), &valid);
        if (!valid || object.value("draft").toBool(true) || object.value("prerelease").toBool(true) ||
            !version(object.value("tag_name").toString()).pre.isEmpty()) {
            setState(Failed, tr("The application or release version is invalid."));
            return;
        }
        if (comparison <= 0) {
            setState(Current, tr("Bitfox Board %1 is up to date.").arg(QCoreApplication::applicationVersion()));
            return;
        }
        QString error;
        if (!parseRelease(object, platformSuffix(), &m_release, &error)) {
            setState(Failed, error);
            return;
        }
        QSettings settings;
        const qint64 deferred = settings.value("appUpdates/deferredUntil", 0).toLongLong();
        const qint64 now = QDateTime::currentSecsSinceEpoch();
        if (!manual && settings.value("appUpdates/deferredTag").toString() == m_release.tag &&
            deferred > now && deferred <= now + 7 * 24 * 60 * 60) {
            setState(Deferred, tr("The application update has been postponed."));
            return;
        }
        setState(Available, tr("Bitfox Board %1 is available.").arg(m_release.version));
    });
}

void AppUpdater::download() {
    if (busy() || m_release.assetName.isEmpty()) return;
    bool valid = false;
    if (compareVersions(m_release.version, QCoreApplication::applicationVersion(), &valid) <= 0 || !valid) {
        setState(Failed, tr("Installing an older or equal application version is not allowed."));
        return;
    }
    setState(Downloading, tr("Downloading and verifying the application package..."));
    if (!m_release.digest.isEmpty()) downloadPackage(m_release.digest);
    else getText(m_release.checksumUrl, 256 * 1024, [this](const QByteArray &body) {
        const QByteArray digest = parseChecksum(body, m_release.assetName);
        if (digest.size() != 32) {
            setState(Failed, tr("The release checksum is missing or invalid."));
            return;
        }
        downloadPackage(digest);
    });
}

void AppUpdater::verifyPackage(const QString &path, qint64 size, const QByteArray &digest,
                               std::function<void(bool)> complete, const QString &destination,
                               const QString &ownedDirectory) {
    auto io = std::make_shared<PackageCacheIO>(path, destination, ownedDirectory, std::move(complete));
    const QFileInfo info(path);
    if (info.isSymLink() || !info.isFile() || info.size() != size || !io->input.open(QIODevice::ReadOnly)) {
        io->complete(false);
        return;
    }
    if (!destination.isEmpty()) {
        if (!io->output.open(QIODevice::WriteOnly | QIODevice::NewOnly)) {
            io->complete(false);
            return;
        }
        io->outputCreated = true;
    }
    auto read = std::make_shared<qint64>(0);
    auto hash = std::make_shared<QCryptographicHash>(QCryptographicHash::Sha256);
    auto *timer = new QTimer(this);
    m_cacheVerification = timer;
    const auto finish = [this, timer, io](bool valid) {
        io->input.close();
        io->output.close();
        io->retained = valid;
        timer->stop();
        timer->deleteLater();
        m_cacheVerification = nullptr;
        io->complete(valid);
    };
    connect(timer, &QTimer::timeout, this, [this, timer, io, read, hash, size, digest, finish] {
        if (m_cacheVerification != timer) {
            timer->stop();
            timer->deleteLater();
            return;
        }
        for (int block = 0; block < 16; ++block) {
            const QByteArray bytes = io->input.read(qMin<qint64>(65536, size - *read + 1));
            if (bytes.isEmpty()) {
                bool valid = io->input.atEnd() && io->input.error() == QFileDevice::NoError &&
                             *read == size && hash->result() == digest;
                if (io->outputCreated) {
                    valid = valid && io->output.flush();
                    io->output.close();
                    valid = valid && io->output.error() == QFileDevice::NoError;
                }
                finish(valid);
                return;
            }
            if (bytes.size() > size - *read ||
                (io->outputCreated && io->output.write(bytes) != bytes.size())) {
                finish(false);
                return;
            }
            *read += bytes.size();
            hash->addData(bytes);
        }
    });
    timer->start(0);
}

void AppUpdater::downloadPackage(const QByteArray &digest) {
    const QString cache = m_cacheDirectory.isEmpty()
                              ? QStandardPaths::writableLocation(QStandardPaths::CacheLocation)
                              : m_cacheDirectory;
    if (cache.isEmpty() || !QDir().mkpath(cache)) {
        setState(Failed, tr("Could not create the application update cache."));
        return;
    }
    const QString lockPath = QDir(cache).filePath("bitfox-app-updater.lock");
    if (QFileInfo(lockPath).isSymLink()) {
        setState(Failed, tr("The application update cache lock is invalid."));
        return;
    }
    auto lock = std::make_shared<QLockFile>(lockPath);
    lock->setStaleLockTime(0);
    if (lock->tryLock(0)) {
        downloadPackageLocked(digest, cache, lock);
        return;
    }
    if (lock->error() != QLockFile::LockFailedError) {
        setState(Failed, tr("Could not lock the application update cache."));
        return;
    }
    auto elapsed = std::make_shared<QElapsedTimer>();
    elapsed->start();
    auto *timer = new QTimer(this);
    m_cacheVerification = timer;
    connect(timer, &QTimer::timeout, this, [this, timer, lock, elapsed, digest, cache] {
        if (m_cacheVerification != timer) {
            timer->stop();
            timer->deleteLater();
            return;
        }
        const bool acquired = lock->tryLock(0);
        if (!acquired && lock->error() == QLockFile::LockFailedError && !elapsed->hasExpired(60000)) return;
        timer->stop();
        timer->deleteLater();
        m_cacheVerification = nullptr;
        if (!acquired) {
            setState(Failed, tr("The application update cache is busy or unavailable. Try again later."));
            return;
        }
        downloadPackageLocked(digest, cache, lock);
    });
    timer->start(20);
}

void AppUpdater::downloadPackageLocked(const QByteArray &digest, const QString &cache,
                                      const std::shared_ptr<QLockFile> &lock) {
    const QByteArray cacheKey = QCryptographicHash::hash(digest + m_release.assetName.toUtf8(), QCryptographicHash::Sha256).toHex();
    const QString directoryName = "verified-" + QString::fromLatin1(cacheKey);
    const QString finalDirectory = QDir(cache).filePath(directoryName);
    const QString finalPath = QDir(finalDirectory).filePath(m_release.assetName);
    const QFileInfo cachedDirectory(finalDirectory);
    if (cachedDirectory.exists() || cachedDirectory.isSymLink()) {
        if (cachedDirectory.isSymLink() || !cachedDirectory.isDir()) {
            setState(Failed, tr("The verified update cache is invalid. Its contents were preserved."));
            return;
        }
        verifyPackage(finalPath, m_release.size, digest, [this, finalPath, lock](bool valid) {
            Q_UNUSED(lock);
            if (!valid) {
                setState(Failed, tr("The cached package failed size or SHA-256 verification. It was preserved."));
                return;
            }
            m_packagePath = finalPath;
            setState(Ready, tr("The package is verified. Open it to install the application using your system."));
        });
        return;
    }
    auto directory = std::make_shared<QTemporaryDir>(QDir(cache).filePath("update-XXXXXX"));
    if (!directory->isValid()) {
        setState(Failed, tr("Could not create a private update directory."));
        return;
    }
    const QString path = directory->filePath(m_release.assetName);
    auto output = std::shared_ptr<QSaveFile>(new QSaveFile(path), [directory](QSaveFile *file) {
        Q_UNUSED(directory);
        delete file;
    });
    output->setDirectWriteFallback(false);
    if (!output->open(QIODevice::WriteOnly)) {
        setState(Failed, tr("Could not save the application package."));
        return;
    }
    const qint64 expected = m_release.size;
    auto written = std::make_shared<qint64>(0);
    auto hash = std::make_shared<QCryptographicHash>(QCryptographicHash::Sha256);
    QNetworkReply *reply = request(m_release.assetUrl, 10 * 60 * 1000);
    auto consume = [reply, output, written, hash, expected] {
        while (reply->bytesAvailable() > 0) {
            QByteArray chunk = reply->read(qMin<qint64>(65536, expected - *written + 1));
            if (chunk.isEmpty() || chunk.size() > expected - *written || output->write(chunk) != chunk.size()) {
                reply->setProperty("updateError", tr("The package size is invalid or storage is unavailable."));
                reply->abort();
                return;
            }
            *written += chunk.size();
            hash->addData(chunk);
        }
    };
    connect(reply, &QNetworkReply::readyRead, this, consume);
    connect(reply, &QNetworkReply::downloadProgress, this, [this, reply, expected](qint64 received, qint64 total) {
        if (received > expected || total > expected) {
            reply->setProperty("updateError", tr("The package exceeds its published size."));
            reply->abort();
            return;
        }
        emit progress(received, expected);
    });
    connect(reply, &QNetworkReply::finished, this, [this, reply, consume, directory, output, written, hash, expected, digest, path, finalDirectory, finalPath, lock] {
        reply->deleteLater();
        if (m_reply != reply) return;
        m_reply = nullptr;
        if (m_state == Cancelled) return;
        consume();
        QString error = replyError(reply);
        if (error.isEmpty() && (*written != expected || hash->result() != digest))
            error = tr("The application package failed size or SHA-256 verification.");
        if (!error.isEmpty()) {
            output->cancelWriting();
            setState(Failed, error);
            return;
        }
        if (!output->commit()) {
            setState(Failed, tr("Could not finish saving the verified application package."));
            return;
        }
        if (!QFile::setPermissions(directory->path(), QFile::ReadOwner | QFile::WriteOwner | QFile::ExeOwner)) {
            setState(Failed, tr("Could not create a private verified update directory."));
            return;
        }
        verifyPackage(path, expected, digest, [this, finalDirectory, finalPath, directory, lock](bool valid) {
            Q_UNUSED(lock);
            const QFileInfo existing(finalDirectory);
            if (!valid || existing.exists() || existing.isSymLink() ||
                !QDir().rename(directory->path(), finalDirectory)) {
                setState(Failed, tr("Could not retain the verified application package without replacing existing cache data."));
                return;
            }
            directory->setAutoRemove(false);
            m_packagePath = finalPath;
            setState(Ready, tr("The package is verified. Open it to install the application using your system."));
        });
    });
}

void AppUpdater::cancel() {
    if (!busy()) return;
    QPointer<QNetworkReply> reply = m_reply;
    m_reply = nullptr;
    if (m_cacheVerification) {
        m_cacheVerification->stop();
        m_cacheVerification->deleteLater();
        m_cacheVerification = nullptr;
    }
    setState(Cancelled, tr("Application update cancelled."));
    if (reply) { reply->abort(); reply->deleteLater(); }
}

void AppUpdater::defer() {
    if (busy()) cancel();
    if (m_release.tag.isEmpty()) return;
    QSettings settings;
    settings.setValue("appUpdates/deferredTag", m_release.tag);
    settings.setValue("appUpdates/deferredUntil", QDateTime::currentSecsSinceEpoch() + 7 * 24 * 60 * 60);
    setState(Deferred, tr("This application update will be offered again in seven days."));
}
