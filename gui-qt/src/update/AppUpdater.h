#pragma once

#include <QObject>
#include <QPointer>
#include <QString>
#include <functional>
#include <memory>

class QJsonObject;
class QNetworkAccessManager;
class QNetworkReply;
class QUrl;
class QLockFile;
class QTimer;

struct AppRelease {
    QString tag;
    QString version;
    QString notes;
    QString assetName;
    QString assetUrl;
    QString checksumUrl;
    QByteArray digest;
    qint64 size = 0;
};

class AppUpdater : public QObject {
    Q_OBJECT
public:
    enum State {
        Idle, Checking, Current, Available, Deferred, Downloading, Ready,
        Cancelled, Offline, RateLimited, Failed, Unsupported
    };
    Q_ENUM(State)

    explicit AppUpdater(QObject *parent = nullptr, QNetworkAccessManager *network = nullptr);
    ~AppUpdater() override;
    State state() const { return m_state; }
    QString message() const { return m_message; }
    AppRelease release() const { return m_release; }
    QString packagePath() const { return m_packagePath; }
    bool busy() const { return m_state == Checking || m_state == Downloading; }
    void check(bool manual);
    void download();
    void cancel();
    void defer();

    static int compareVersions(const QString &left, const QString &right, bool *valid = nullptr);
    static QString platformSuffix();
    static bool parseRelease(const QJsonObject &object, const QString &suffix,
                             AppRelease *release, QString *error);
    static QByteArray parseChecksum(const QByteArray &body, const QString &assetName);
    static bool trustedUrl(const QUrl &url);

signals:
    void stateChanged();
    void progress(qint64 received, qint64 total);

private:
    friend class AppUpdaterTest;
    QString m_cacheDirectory;
    void setState(State state, const QString &message);
    QNetworkReply *request(const QString &url, int timeoutMs);
    QString replyError(QNetworkReply *reply) const;
    void getText(const QString &url, qsizetype limit,
                 std::function<void(const QByteArray &)> complete);
    void downloadPackage(const QByteArray &digest);
    void downloadPackageLocked(const QByteArray &digest, const QString &cache,
                               const std::shared_ptr<QLockFile> &lock);
    void verifyPackage(const QString &path, qint64 size, const QByteArray &digest,
                       std::function<void(bool)> complete, const QString &destination = {},
                       const QString &ownedDirectory = {});

    QNetworkAccessManager *m_network;
    QPointer<QNetworkReply> m_reply;
    QPointer<QTimer> m_cacheVerification;
    State m_state = Idle;
    QString m_message;
    AppRelease m_release;
    QString m_packagePath;
};
