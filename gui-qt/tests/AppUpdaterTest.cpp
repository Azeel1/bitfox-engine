#include <QCryptographicHash>
#include <QDateTime>
#include <QDirIterator>
#include <QFile>
#include <QFileInfo>
#include <QJsonArray>
#include <QJsonDocument>
#include <QJsonObject>
#include <QNetworkAccessManager>
#include <QNetworkReply>
#include <QProcess>
#include <QProcessEnvironment>
#include <QSettings>
#include <QSignalSpy>
#include <QTemporaryDir>
#include <QTimer>
#include <QTimerEvent>
#include <QtTest>
#include <cstdlib>
#include <cstring>

#include "update/AppUpdater.h"

namespace {
struct Response {
    Response(QByteArray bytes = {}, int code = 200,
             QNetworkReply::NetworkError failure = QNetworkReply::NoError,
             bool wait = false, QUrl target = {})
        : body(std::move(bytes)), status(code), error(failure), hold(wait), redirect(std::move(target)) {}
    QByteArray body;
    int status = 200;
    QNetworkReply::NetworkError error = QNetworkReply::NoError;
    bool hold = false;
    QUrl redirect;
};

class FakeReply : public QNetworkReply {
public:
    FakeReply(const QNetworkRequest &request, Response response, QObject *parent)
        : QNetworkReply(parent), m_response(std::move(response)) {
        setRequest(request);
        setUrl(request.url());
        setAttribute(QNetworkRequest::HttpStatusCodeAttribute, m_response.status);
        open(QIODevice::ReadOnly);
        QTimer::singleShot(0, this, [this] {
            if (isFinished()) return;
            if (!m_response.redirect.isEmpty()) {
                emit redirected(m_response.redirect);
                return;
            }
            if (m_response.hold) return;
            if (m_response.error != NoError) setError(m_response.error, "Fixture network error");
            emit readyRead();
            if (isFinished()) return;
            setFinished(true);
            emit finished();
        });
    }
    void abort() override {
        if (isFinished()) return;
        setError(OperationCanceledError, "Cancelled");
        setFinished(true);
        emit finished();
    }
    qint64 bytesAvailable() const override {
        return m_response.body.size() - m_position + QNetworkReply::bytesAvailable();
    }
protected:
    qint64 readData(char *data, qint64 maximum) override {
        const qint64 size = qMin<qint64>(maximum, m_response.body.size() - m_position);
        if (size <= 0) return -1;
        std::memcpy(data, m_response.body.constData() + m_position, size_t(size));
        m_position += size;
        return size;
    }
private:
    Response m_response;
    qint64 m_position = 0;
};

class FakeNetwork : public QNetworkAccessManager {
public:
    QList<Response> responses;
    QList<QNetworkRequest> requests;
    QNetworkReply *lastReply = nullptr;
protected:
    QNetworkReply *createRequest(Operation, const QNetworkRequest &request, QIODevice *) override {
        requests.append(request);
        lastReply = new FakeReply(request, responses.isEmpty() ? Response{{}, 500} : responses.takeFirst(), this);
        return lastReply;
    }
};

QJsonObject fixture(const QByteArray &package = QByteArrayLiteral("verified archive")) {
    const QString tag = QStringLiteral("v2.10.0");
    const QString name = "bitfox-2.10.0" + AppUpdater::platformSuffix();
    const QString base = "https://github.com/Azeel1/bitfox-engine/releases/download/" + tag + '/';
    return {{"tag_name", tag}, {"draft", false}, {"prerelease", false}, {"body", "Changes\nNew application features"},
            {"assets", QJsonArray{QJsonObject{{"name", name}, {"size", package.size()},
                    {"digest", "sha256:" + QString::fromLatin1(QCryptographicHash::hash(package, QCryptographicHash::Sha256).toHex())},
                    {"browser_download_url", base + name}}}}};
}

QByteArray encoded(const QJsonObject &object) { return QJsonDocument(object).toJson(); }
}

class AppUpdaterTest : public QObject {
    Q_OBJECT
private slots:
    void initTestCase();
    void init();
    void versions();
    void metadata();
    void checksums();
    void verifiedDownload();
    void corruptedDownloadIsRemoved();
    void publishedSizeIsEnforced();
    void checksumFallback();
    void networkStates();
    void cancellationAndLateReply();
    void deferAndManualCheck();
    void currentVersionNeverDownloads();
    void untrustedRedirectIsRejected();
    void responseLimitAndDestruction();
    void repeatedVerifiedDownloadsReuseCache();
    void cachedPackageIsReverifiedAndPreserved();
    void cancellationPreservesVerifiedCache();
    void finalDirectorySymlinkIsRejected();
    void concurrentUpdatesRetainOneArchive();
    void cacheLockIsReleasedAfterCancellationAndDestruction();
    void partialCacheCopyIsRemovedOnlyWhenOwned();
    void interruptedVerificationDoesNotPublishCache();
    void promotionPreservesUnexpectedDirectory();
private:
    QTemporaryDir m_work;
};

void AppUpdaterTest::initTestCase() {
    QVERIFY(m_work.isValid());
    QSettings::setDefaultFormat(QSettings::IniFormat);
    QSettings::setPath(QSettings::IniFormat, QSettings::UserScope, m_work.path());
    QCoreApplication::setOrganizationName("BitfoxUpdaterTest");
    QCoreApplication::setApplicationName("UpdaterTest");
    QVERIFY(!AppUpdater::platformSuffix().isEmpty());
}

void AppUpdaterTest::init() {
    QSettings().clear();
    QCoreApplication::setApplicationVersion("2.5.0");
}

void AppUpdaterTest::versions() {
    bool valid = false;
    QCOMPARE(AppUpdater::compareVersions("v2.10.0", "2.9.9", &valid), 1);
    QVERIFY(valid);
    QCOMPARE(AppUpdater::compareVersions("2.5.0", "2.5.0+build.8", &valid), 0);
    QCOMPARE(AppUpdater::compareVersions("2.5.0", "2.5.0-rc.9", &valid), 1);
    QCOMPARE(AppUpdater::compareVersions("2.5.0-rc.10", "2.5.0-rc.9", &valid), 1);
    QCOMPARE(AppUpdater::compareVersions("2.5.0-1", "2.5.0-alpha", &valid), -1);
    QCOMPARE(AppUpdater::compareVersions("999999999999999999999.0.0", "3.0.0", &valid), 1);
    QVERIFY(valid);
    for (const QString bad : {"2.05.0", "2.5", "2.5.0-01", "2.5.0/../x", "2.5.0+"}) {
        AppUpdater::compareVersions(bad, "2.5.0", &valid);
        QVERIFY2(!valid, qPrintable(bad));
    }
}

void AppUpdaterTest::metadata() {
    AppRelease release;
    QString error;
    QJsonObject object = fixture();
    QVERIFY(AppUpdater::parseRelease(object, AppUpdater::platformSuffix(), &release, &error));
    QVERIFY(release.assetName.contains("-gui."));
    QCOMPARE(release.digest.size(), 32);
    QJsonObject asset = object["assets"].toArray().first().toObject();
    for (const QString url : {"https://example.com/evil", "https://user@github.com/evil", "https://github.com:444/evil"}) {
        QJsonObject badAsset = asset;
        badAsset["browser_download_url"] = url;
        object["assets"] = QJsonArray{badAsset};
        QVERIFY(!AppUpdater::parseRelease(object, AppUpdater::platformSuffix(), &release, &error));
    }
    object = fixture();
    object["assets"] = QJsonArray{asset, asset};
    QVERIFY(!AppUpdater::parseRelease(object, AppUpdater::platformSuffix(), &release, &error));
    object = fixture();
    object["prerelease"] = true;
    QVERIFY(!AppUpdater::parseRelease(object, AppUpdater::platformSuffix(), &release, &error));
    object = fixture();
    asset["size"] = 0;
    object["assets"] = QJsonArray{asset};
    QVERIFY(!AppUpdater::parseRelease(object, AppUpdater::platformSuffix(), &release, &error));
    QVERIFY(!AppUpdater::parseRelease(fixture(), "-wrong-arch.zip", &release, &error));
}

void AppUpdaterTest::checksums() {
    const QByteArray digest(64, 'a');
    QCOMPARE(AppUpdater::parseChecksum(digest + "  package.zip\n", "package.zip"), QByteArray::fromHex(digest));
    QCOMPARE(AppUpdater::parseChecksum(digest + " *package.zip\r\n", "package.zip"), QByteArray::fromHex(digest));
    QVERIFY(AppUpdater::parseChecksum(digest + "  other.zip\n", "package.zip").isEmpty());
    QVERIFY(AppUpdater::parseChecksum(digest + "  package.zip\n" + digest + "  package.zip\n", "package.zip").isEmpty());
    QVERIFY(AppUpdater::parseChecksum("broken\n" + digest + "  package.zip\n", "package.zip").isEmpty());
}

void AppUpdaterTest::verifiedDownload() {
    FakeNetwork network;
    const QByteArray bytes("verified archive");
    network.responses = {{encoded(fixture(bytes))}, {bytes}};
    AppUpdater updater(nullptr, &network);
    QTemporaryDir cache(m_work.filePath("cache-XXXXXX"));
    updater.m_cacheDirectory = cache.path();
    updater.check(true);
    QTRY_COMPARE(updater.state(), AppUpdater::Available);
    updater.download();
    QTRY_COMPARE(updater.state(), AppUpdater::Ready);
    QFile file(updater.packagePath());
    QVERIFY(file.open(QIODevice::ReadOnly));
    QCOMPARE(file.readAll(), bytes);
    QCOMPARE(network.requests.size(), 2);
    QCOMPARE(network.requests.first().attribute(QNetworkRequest::RedirectPolicyAttribute).toInt(), int(QNetworkRequest::UserVerifiedRedirectPolicy));
}

void AppUpdaterTest::corruptedDownloadIsRemoved() {
    FakeNetwork network;
    const QByteArray bytes("verified archive");
    network.responses = {{encoded(fixture(bytes))}, {QByteArray(bytes.size(), 'x')}};
    AppUpdater updater(nullptr, &network);
    QTemporaryDir cache(m_work.filePath("cache-XXXXXX"));
    updater.m_cacheDirectory = cache.path();
    updater.check(true);
    QTRY_COMPARE(updater.state(), AppUpdater::Available);
    updater.download();
    QTRY_COMPARE(updater.state(), AppUpdater::Failed);
    QCoreApplication::sendPostedEvents(nullptr, QEvent::DeferredDelete);
    QVERIFY(updater.packagePath().isEmpty());
    QVERIFY(QDir(cache.path()).entryList(QDir::AllEntries | QDir::NoDotAndDotDot).isEmpty());
}

void AppUpdaterTest::publishedSizeIsEnforced() {
    FakeNetwork network;
    network.responses = {{encoded(fixture("short"))}, {QByteArray(65536, 'x')}};
    AppUpdater updater(nullptr, &network);
    QTemporaryDir cache(m_work.filePath("cache-XXXXXX"));
    updater.m_cacheDirectory = cache.path();
    updater.check(true);
    QTRY_COMPARE(updater.state(), AppUpdater::Available);
    updater.download();
    QTRY_COMPARE(updater.state(), AppUpdater::Failed);
    QVERIFY(updater.packagePath().isEmpty());
}

void AppUpdaterTest::checksumFallback() {
    FakeNetwork network;
    const QByteArray bytes("verified archive");
    QJsonObject object = fixture(bytes);
    QJsonObject asset = object["assets"].toArray().first().toObject();
    asset.remove("digest");
    const QString base = "https://github.com/Azeel1/bitfox-engine/releases/download/v2.10.0/";
    object["assets"] = QJsonArray{asset, QJsonObject{{"name", "SHA256SUMS"}, {"browser_download_url", base + "SHA256SUMS"}}};
    const QByteArray sum = QCryptographicHash::hash(bytes, QCryptographicHash::Sha256).toHex() + "  " + asset["name"].toString().toUtf8() + '\n';
    network.responses = {{encoded(object)}, {sum}, {bytes}};
    AppUpdater updater(nullptr, &network);
    QTemporaryDir cache(m_work.filePath("cache-XXXXXX"));
    updater.m_cacheDirectory = cache.path();
    updater.check(true);
    QTRY_COMPARE(updater.state(), AppUpdater::Available);
    updater.download();
    QTRY_COMPARE(updater.state(), AppUpdater::Ready);
    QCOMPARE(network.requests.size(), 3);
}

void AppUpdaterTest::networkStates() {
    FakeNetwork network;
    AppUpdater updater(nullptr, &network);
    network.responses = {{{}, 403, QNetworkReply::ContentAccessDenied}, {{}, 0, QNetworkReply::HostNotFoundError}, {"not json"}};
    updater.check(true);
    QTRY_COMPARE(updater.state(), AppUpdater::RateLimited);
    updater.check(true);
    QTRY_COMPARE(updater.state(), AppUpdater::Offline);
    updater.check(true);
    QTRY_COMPARE(updater.state(), AppUpdater::Failed);
}

void AppUpdaterTest::cancellationAndLateReply() {
    FakeNetwork network;
    network.responses = {{{}, 200, QNetworkReply::NoError, true}, {encoded(fixture())}, {{}, 200, QNetworkReply::NoError, true}};
    AppUpdater updater(nullptr, &network);
    QTemporaryDir cache(m_work.filePath("cache-XXXXXX"));
    updater.m_cacheDirectory = cache.path();
    updater.check(true);
    updater.cancel();
    QCOMPARE(updater.state(), AppUpdater::Cancelled);
    updater.check(true);
    QTRY_COMPARE(updater.state(), AppUpdater::Available);
    updater.download();
    QCOMPARE(updater.state(), AppUpdater::Downloading);
    updater.cancel();
    QCoreApplication::sendPostedEvents(nullptr, QEvent::DeferredDelete);
    QCOMPARE(updater.state(), AppUpdater::Cancelled);
    QVERIFY(QDir(cache.path()).entryList(QDir::AllEntries | QDir::NoDotAndDotDot).isEmpty());
}

void AppUpdaterTest::deferAndManualCheck() {
    FakeNetwork network;
    network.responses = {{encoded(fixture())}, {encoded(fixture())}, {encoded(fixture())}};
    AppUpdater updater(nullptr, &network);
    updater.check(true);
    QTRY_COMPARE(updater.state(), AppUpdater::Available);
    updater.defer();
    QCOMPARE(updater.state(), AppUpdater::Deferred);
    QSettings().setValue("appUpdates/lastCheck", QDateTime::currentSecsSinceEpoch() - 86401);
    updater.check(false);
    QTRY_COMPARE(updater.state(), AppUpdater::Deferred);
    updater.check(true);
    QTRY_COMPARE(updater.state(), AppUpdater::Available);
}

void AppUpdaterTest::currentVersionNeverDownloads() {
    FakeNetwork network;
    network.responses = {{encoded(fixture())}};
    QCoreApplication::setApplicationVersion("3.0.0");
    AppUpdater updater(nullptr, &network);
    updater.check(true);
    QTRY_COMPARE(updater.state(), AppUpdater::Current);
    updater.download();
    QCOMPARE(network.requests.size(), 1);
}

void AppUpdaterTest::untrustedRedirectIsRejected() {
    FakeNetwork network;
    network.responses = {{{}, 200, QNetworkReply::NoError, false, QUrl("https://example.com/update")}};
    AppUpdater updater(nullptr, &network);
    updater.check(true);
    QTRY_COMPARE(updater.state(), AppUpdater::Failed);
    QVERIFY(updater.message().contains("untrusted"));
    QVERIFY(!AppUpdater::trustedUrl(QUrl("http://github.com/file")));
    QVERIFY(!AppUpdater::trustedUrl(QUrl("https://github.com.evil.invalid/file")));
    QVERIFY(!AppUpdater::trustedUrl(QUrl("https://user:pass@github.com/file")));
    QVERIFY(AppUpdater::trustedUrl(QUrl("https://release-assets.githubusercontent.com/file")));
}

void AppUpdaterTest::responseLimitAndDestruction() {
    FakeNetwork network;
    network.responses = {{QByteArray(4 * 1024 * 1024 + 1, 'x')}, {encoded(fixture())}, {{}, 200, QNetworkReply::NoError, true}};
    auto *updater = new AppUpdater(nullptr, &network);
    QTemporaryDir cache(m_work.filePath("cache-XXXXXX"));
    updater->m_cacheDirectory = cache.path();
    updater->check(true);
    QTRY_COMPARE(updater->state(), AppUpdater::Failed);
    QVERIFY(updater->message().contains("too large"));
    updater->check(true);
    QTRY_COMPARE(updater->state(), AppUpdater::Available);
    updater->download();
    QCOMPARE(updater->state(), AppUpdater::Downloading);
    delete updater;
    QCoreApplication::sendPostedEvents(nullptr, QEvent::DeferredDelete);
    QVERIFY(QDir(cache.path()).entryList(QDir::AllEntries | QDir::NoDotAndDotDot).isEmpty());
}

void AppUpdaterTest::repeatedVerifiedDownloadsReuseCache() {
    FakeNetwork network;
    const QByteArray bytes("verified archive");
    network.responses = {{encoded(fixture(bytes))}, {bytes}, {encoded(fixture(bytes))}};
    QTemporaryDir cache(m_work.filePath("cache-XXXXXX"));
    QVERIFY(QDir().mkpath(cache.filePath("update-previous")));
    QFile unrelated(cache.filePath("update-previous/installer-package.zip"));
    QVERIFY(unrelated.open(QIODevice::WriteOnly));
    QCOMPARE(unrelated.write("previous package"), qint64(16));
    unrelated.close();
    QString saved;
    {
        AppUpdater updater(nullptr, &network);
        updater.m_cacheDirectory = cache.path();
        updater.check(true);
        QTRY_COMPARE(updater.state(), AppUpdater::Available);
        updater.download();
        QTRY_COMPARE(updater.state(), AppUpdater::Ready);
        saved = updater.packagePath();
        QFile installer(saved);
        QVERIFY(installer.open(QIODevice::ReadOnly));
        updater.check(true);
        QTRY_COMPARE(updater.state(), AppUpdater::Available);
        updater.download();
        QTRY_COMPARE(updater.state(), AppUpdater::Ready);
        QCOMPARE(updater.packagePath(), saved);
        QCOMPARE(network.requests.size(), 3);
        QCOMPARE(installer.readAll(), bytes);
    }
    network.responses = {{encoded(fixture(bytes))}};
    AppUpdater next(nullptr, &network);
    next.m_cacheDirectory = cache.path();
    next.check(true);
    QTRY_COMPARE(next.state(), AppUpdater::Available);
    next.download();
    QTRY_COMPARE(next.state(), AppUpdater::Ready);
    QCOMPARE(next.packagePath(), saved);
    QCOMPARE(network.requests.size(), 4);
    QCOMPARE(QDir(cache.path()).entryList(QDir::Dirs | QDir::NoDotAndDotDot).size(), 2);
    QVERIFY(unrelated.open(QIODevice::ReadOnly));
    QCOMPARE(unrelated.readAll(), QByteArray("previous package"));
}

void AppUpdaterTest::cachedPackageIsReverifiedAndPreserved() {
    FakeNetwork network;
    const QByteArray bytes("verified archive");
    network.responses = {{encoded(fixture(bytes))}, {bytes}, {encoded(fixture(bytes))}};
    AppUpdater updater(nullptr, &network);
    QTemporaryDir cache(m_work.filePath("cache-XXXXXX"));
    updater.m_cacheDirectory = cache.path();
    updater.check(true);
    QTRY_COMPARE(updater.state(), AppUpdater::Available);
    updater.download();
    QTRY_COMPARE(updater.state(), AppUpdater::Ready);
    const QString saved = updater.packagePath();
    const QByteArray altered(bytes.size(), 'x');
    QFile file(saved);
    QVERIFY(file.open(QIODevice::WriteOnly | QIODevice::Truncate));
    QCOMPARE(file.write(altered), qint64(altered.size()));
    file.close();
    updater.check(true);
    QTRY_COMPARE(updater.state(), AppUpdater::Available);
    updater.download();
    QTRY_COMPARE(updater.state(), AppUpdater::Failed);
    QVERIFY(updater.message().contains("SHA-256"));
    QCOMPARE(network.requests.size(), 3);
    QVERIFY(file.open(QIODevice::ReadOnly));
    QCOMPARE(file.readAll(), altered);
}

void AppUpdaterTest::cancellationPreservesVerifiedCache() {
    FakeNetwork network;
    const QByteArray bytes(3 * 1024 * 1024, 'x');
    network.responses = {{encoded(fixture(bytes))}, {bytes}, {encoded(fixture(bytes))}};
    AppUpdater updater(nullptr, &network);
    QTemporaryDir cache(m_work.filePath("cache-XXXXXX"));
    updater.m_cacheDirectory = cache.path();
    updater.check(true);
    QTRY_COMPARE(updater.state(), AppUpdater::Available);
    updater.download();
    QTRY_COMPARE(updater.state(), AppUpdater::Ready);
    const QString saved = updater.packagePath();
    updater.check(true);
    QTRY_COMPARE(updater.state(), AppUpdater::Available);
    updater.download();
    QCOMPARE(updater.state(), AppUpdater::Downloading);
    updater.cancel();
    QCoreApplication::sendPostedEvents(nullptr, QEvent::DeferredDelete);
    QTest::qWait(1);
    QCOMPARE(updater.state(), AppUpdater::Cancelled);
    QCOMPARE(network.requests.size(), 3);
    QFile file(saved);
    QVERIFY(file.open(QIODevice::ReadOnly));
    QCOMPARE(file.readAll(), bytes);
}

void AppUpdaterTest::finalDirectorySymlinkIsRejected() {
#if defined(Q_OS_UNIX)
    FakeNetwork network;
    const QByteArray bytes("verified archive");
    const QJsonObject object = fixture(bytes);
    network.responses = {{encoded(object)}};
    QTemporaryDir cache(m_work.filePath("cache-XXXXXX"));
    QTemporaryDir outside(m_work.filePath("outside-XXXXXX"));
    const QString assetName = object["assets"].toArray().first().toObject()["name"].toString();
    const QByteArray digest = QCryptographicHash::hash(bytes, QCryptographicHash::Sha256);
    const QByteArray key = QCryptographicHash::hash(digest + assetName.toUtf8(), QCryptographicHash::Sha256).toHex();
    const QString finalDirectory = cache.filePath("verified-" + QString::fromLatin1(key));
    QFile delivered(outside.filePath(assetName));
    QVERIFY(delivered.open(QIODevice::WriteOnly));
    QCOMPARE(delivered.write(bytes), qint64(bytes.size()));
    delivered.close();
    QVERIFY(QFile::link(outside.path(), finalDirectory));
    QVERIFY(QFileInfo(finalDirectory).isSymLink());
    AppUpdater updater(nullptr, &network);
    updater.m_cacheDirectory = cache.path();
    updater.check(true);
    QTRY_COMPARE(updater.state(), AppUpdater::Available);
    updater.download();
    QTRY_COMPARE(updater.state(), AppUpdater::Failed);
    QVERIFY(updater.message().contains("cache"));
    QCOMPARE(network.requests.size(), 1);
    QVERIFY(QFileInfo(finalDirectory).isSymLink());
    QVERIFY(delivered.open(QIODevice::ReadOnly));
    QCOMPARE(delivered.readAll(), bytes);
#else
    QSKIP("This fixture requires Unix directory symlinks.");
#endif
}

void AppUpdaterTest::concurrentUpdatesRetainOneArchive() {
    const QByteArray bytes(3 * 1024 * 1024, 'x');
    FakeNetwork firstNetwork, secondNetwork;
    firstNetwork.responses = {{encoded(fixture(bytes))}, {bytes}};
    secondNetwork.responses = {{encoded(fixture(bytes))}, {bytes}};
    QTemporaryDir cache(m_work.filePath("cache-XXXXXX"));
    QVERIFY(QDir().mkpath(cache.filePath("update-previous")));
    QFile unrelated(cache.filePath("update-previous/installer-package.zip"));
    QVERIFY(unrelated.open(QIODevice::WriteOnly));
    QCOMPARE(unrelated.write("previous package"), qint64(16));
    unrelated.close();
    AppUpdater first(nullptr, &firstNetwork), second(nullptr, &secondNetwork);
    first.m_cacheDirectory = cache.path();
    second.m_cacheDirectory = cache.path();
    first.check(true);
    second.check(true);
    QTRY_COMPARE(first.state(), AppUpdater::Available);
    QTRY_COMPARE(second.state(), AppUpdater::Available);
    first.download();
    second.download();
    QTRY_COMPARE(first.state(), AppUpdater::Ready);
    QFile installer(first.packagePath());
    QVERIFY(installer.open(QIODevice::ReadOnly));
    QTRY_COMPARE(second.state(), AppUpdater::Ready);
    QCOMPARE(first.packagePath(), second.packagePath());
    QCOMPARE(firstNetwork.requests.size(), 2);
    QCOMPARE(secondNetwork.requests.size(), 1);
    QCOMPARE(installer.readAll(), bytes);
    QTRY_COMPARE(QDir(cache.path()).entryList(QDir::Dirs | QDir::NoDotAndDotDot).size(), 2);
    QVERIFY(unrelated.open(QIODevice::ReadOnly));
    QCOMPARE(unrelated.readAll(), QByteArray("previous package"));
}

void AppUpdaterTest::cacheLockIsReleasedAfterCancellationAndDestruction() {
    for (bool destroy : {false, true}) {
        const QByteArray bytes("verified archive");
        FakeNetwork firstNetwork, secondNetwork;
        firstNetwork.responses = {{encoded(fixture(bytes))}, {{}, 200, QNetworkReply::NoError, true}};
        secondNetwork.responses = {{encoded(fixture(bytes))}, {bytes}};
        QTemporaryDir cache(m_work.filePath("cache-XXXXXX"));
        auto *first = new AppUpdater(nullptr, &firstNetwork);
        first->m_cacheDirectory = cache.path();
        AppUpdater second(nullptr, &secondNetwork);
        second.m_cacheDirectory = cache.path();
        first->check(true);
        second.check(true);
        QTRY_COMPARE(first->state(), AppUpdater::Available);
        QTRY_COMPARE(second.state(), AppUpdater::Available);
        first->download();
        second.download();
        QCOMPARE(first->state(), AppUpdater::Downloading);
        QCOMPARE(second.state(), AppUpdater::Downloading);
        if (destroy) delete first;
        else first->cancel();
        QCoreApplication::sendPostedEvents(nullptr, QEvent::DeferredDelete);
        QTRY_COMPARE(second.state(), AppUpdater::Ready);
        QFile file(second.packagePath());
        QVERIFY(file.open(QIODevice::ReadOnly));
        QCOMPARE(file.readAll(), bytes);
        QTRY_COMPARE(QDir(cache.path()).entryList(QDir::Dirs | QDir::NoDotAndDotDot).size(), 1);
        if (!destroy) delete first;
    }
}

void AppUpdaterTest::partialCacheCopyIsRemovedOnlyWhenOwned() {
    for (bool destroy : {false, true}) {
        const QByteArray bytes(3 * 1024 * 1024, 'x');
        QTemporaryDir work(m_work.filePath("copy-XXXXXX"));
        const QString source = work.filePath("source.zip");
        QFile input(source);
        QVERIFY(input.open(QIODevice::WriteOnly));
        QCOMPARE(input.write(bytes), qint64(bytes.size()));
        input.close();
        const QString directory = work.filePath("owned");
        QVERIFY(QDir().mkdir(directory));
        const QString destination = QDir(directory).filePath("package.zip");
        auto *updater = new AppUpdater;
        updater->m_state = AppUpdater::Downloading;
        bool completed = false;
        updater->verifyPackage(source, bytes.size(), QCryptographicHash::hash(bytes, QCryptographicHash::Sha256),
                               [&completed](bool) { completed = true; }, destination, directory);
        QVERIFY(updater->m_cacheVerification);
        updater->m_cacheVerification->setInterval(10000);
        QTimerEvent tick(updater->m_cacheVerification->timerId());
        QCoreApplication::sendEvent(updater->m_cacheVerification, &tick);
        QCOMPARE(QFileInfo(destination).size(), qint64(1024 * 1024));
        if (destroy) delete updater;
        else updater->cancel();
        QCoreApplication::sendPostedEvents(nullptr, QEvent::DeferredDelete);
        QVERIFY(!completed);
        QVERIFY(!QFileInfo::exists(destination));
        QVERIFY(!QFileInfo::exists(directory));
        QVERIFY(input.open(QIODevice::ReadOnly));
        QCOMPARE(input.readAll(), bytes);
        input.close();
        if (!destroy) delete updater;

        QVERIFY(QDir().mkdir(directory));
        QFile delivered(destination);
        QVERIFY(delivered.open(QIODevice::WriteOnly));
        QCOMPARE(delivered.write("delivered archive"), qint64(17));
        delivered.close();
        AppUpdater refused;
        bool accepted = true;
        refused.verifyPackage(source, bytes.size(), QCryptographicHash::hash(bytes, QCryptographicHash::Sha256),
                              [&accepted](bool valid) { accepted = valid; }, destination, directory);
        QVERIFY(!accepted);
        QVERIFY(delivered.open(QIODevice::ReadOnly));
        QCOMPARE(delivered.readAll(), QByteArray("delivered archive"));
        QVERIFY(QFileInfo::exists(directory));
    }
}

void AppUpdaterTest::interruptedVerificationDoesNotPublishCache() {
    const QByteArray bytes(3 * 1024 * 1024, 'x');
    const QJsonObject object = fixture(bytes);
    const QString assetName = object["assets"].toArray().first().toObject()["name"].toString();
    const QByteArray digest = QCryptographicHash::hash(bytes, QCryptographicHash::Sha256);
    const QByteArray key = QCryptographicHash::hash(digest + assetName.toUtf8(), QCryptographicHash::Sha256).toHex();
    const QString childCache = qEnvironmentVariable("BITFOX_UPDATER_CRASH_CACHE");
    if (!childCache.isEmpty()) {
        FakeNetwork network;
        network.responses = {{encoded(object)}, {bytes}};
        AppUpdater updater(nullptr, &network);
        updater.m_cacheDirectory = childCache;
        updater.check(true);
        QTRY_COMPARE(updater.state(), AppUpdater::Available);
        updater.download();
        QVERIFY(network.lastReply);
        bool verificationStarted = false;
        connect(network.lastReply, &QNetworkReply::finished, &updater, [&] {
            if (!updater.m_cacheVerification) return;
            updater.m_cacheVerification->setInterval(10000);
            QTimerEvent tick(updater.m_cacheVerification->timerId());
            QCoreApplication::sendEvent(updater.m_cacheVerification, &tick);
            verificationStarted = true;
        });
        QTRY_VERIFY(verificationStarted);
        QCOMPARE(updater.state(), AppUpdater::Downloading);
        std::_Exit(23);
    }

    QTemporaryDir cache(m_work.filePath("crash-cache-XXXXXX"));
    QVERIFY(cache.isValid());
    const QString finalDirectory = cache.filePath("verified-" + QString::fromLatin1(key));
    QProcess child;
    QProcessEnvironment environment = QProcessEnvironment::systemEnvironment();
    environment.insert("BITFOX_UPDATER_CRASH_CACHE", cache.path());
    environment.insert("TMPDIR", m_work.path());
    environment.insert("TEMP", m_work.path());
    environment.insert("TMP", m_work.path());
    child.setProcessEnvironment(environment);
    child.setProcessChannelMode(QProcess::MergedChannels);
    child.start(QCoreApplication::applicationFilePath(), {"interruptedVerificationDoesNotPublishCache"});
    QVERIFY(child.waitForStarted(5000));
    QVERIFY(child.waitForFinished(15000));
    const QByteArray childOutput = child.readAll();
    QVERIFY2(child.exitStatus() == QProcess::NormalExit, childOutput.constData());
    QVERIFY2(child.exitCode() == 23, childOutput.constData());
    QVERIFY(!QFileInfo::exists(finalDirectory));
    const QStringList abandoned = QDir(cache.path()).entryList({"update-*"}, QDir::Dirs | QDir::NoDotAndDotDot);
    QCOMPARE(abandoned.size(), 1);
    const QString abandonedPath = QDir(cache.filePath(abandoned.first())).filePath(assetName);
    QFile abandonedPackage(abandonedPath);
    QVERIFY(abandonedPackage.open(QIODevice::ReadOnly));
    QCOMPARE(abandonedPackage.readAll(), bytes);
    abandonedPackage.close();

    FakeNetwork network;
    network.responses = {{encoded(object)}, {bytes}};
    AppUpdater updater(nullptr, &network);
    updater.m_cacheDirectory = cache.path();
    updater.check(true);
    QTRY_COMPARE(updater.state(), AppUpdater::Available);
    updater.download();
    QTRY_COMPARE(updater.state(), AppUpdater::Ready);
    QCOMPARE(updater.packagePath(), QDir(finalDirectory).filePath(assetName));
    QFile package(updater.packagePath());
    QVERIFY(package.open(QIODevice::ReadOnly));
    QCOMPARE(package.readAll(), bytes);
    QVERIFY(abandonedPackage.open(QIODevice::ReadOnly));
    QCOMPARE(abandonedPackage.readAll(), bytes);
    QCOMPARE(network.requests.size(), 2);
}

void AppUpdaterTest::promotionPreservesUnexpectedDirectory() {
    const QByteArray bytes(3 * 1024 * 1024, 'x');
    const QJsonObject object = fixture(bytes);
    const QString assetName = object["assets"].toArray().first().toObject()["name"].toString();
    const QByteArray digest = QCryptographicHash::hash(bytes, QCryptographicHash::Sha256);
    const QByteArray key = QCryptographicHash::hash(digest + assetName.toUtf8(), QCryptographicHash::Sha256).toHex();
    QTemporaryDir cache(m_work.filePath("conflict-cache-XXXXXX"));
    QVERIFY(cache.isValid());
    const QString finalDirectory = cache.filePath("verified-" + QString::fromLatin1(key));
    const QString markerPath = QDir(finalDirectory).filePath("existing.txt");
    FakeNetwork network;
    network.responses = {{encoded(object)}, {bytes}};
    AppUpdater updater(nullptr, &network);
    updater.m_cacheDirectory = cache.path();
    updater.check(true);
    QTRY_COMPARE(updater.state(), AppUpdater::Available);
    updater.download();
    QVERIFY(network.lastReply);
    bool created = false;
    connect(network.lastReply, &QNetworkReply::finished, &updater, [&] {
        if (!QDir().mkdir(finalDirectory)) return;
        QFile marker(markerPath);
        if (!marker.open(QIODevice::WriteOnly | QIODevice::NewOnly)) return;
        created = marker.write("previous cache") == qint64(14);
    });
    QTRY_COMPARE(updater.state(), AppUpdater::Failed);
    QVERIFY(created);
    QVERIFY(!QFileInfo::exists(QDir(finalDirectory).filePath(assetName)));
    QFile marker(markerPath);
    QVERIFY(marker.open(QIODevice::ReadOnly));
    QCOMPARE(marker.readAll(), QByteArray("previous cache"));
    QCoreApplication::sendPostedEvents(nullptr, QEvent::DeferredDelete);
    QCOMPARE(QDir(cache.path()).entryList({"update-*"}, QDir::Dirs | QDir::NoDotAndDotDot).size(), 0);
}

QTEST_GUILESS_MAIN(AppUpdaterTest)
#include "AppUpdaterTest.moc"
