#pragma once

#include <QObject>
#include <QSet>
#include <QString>
#include <QVector>

#include <memory>
#include <functional>

class QByteArray;
class QNetworkAccessManager;
class QNetworkReply;
class QTemporaryDir;
class ReleaseManagerTest;
class QJsonObject;

struct ReleaseInfo {
    QString tag;
    QString name;
    QString assetUrl;
    QString assetName;
    QString checksumUrl;
    bool installed = false;
};

class ReleaseManager : public QObject {
    Q_OBJECT
public:
    explicit ReleaseManager(QObject *parent = nullptr, const QString &engineDirectory = {});
    ~ReleaseManager() override;

    void fetchReleases();
    void downloadRelease(const ReleaseInfo &info);
    static QString enginesDir();

signals:
    void releasesReady(const QVector<ReleaseInfo> &releases);
    void fetchFailed(const QString &error);
    void downloadProgress(const QString &tag, qint64 received, qint64 total);
    void downloadFinished(const QString &tag, bool success, const QString &error);

private:
    friend class ReleaseManagerTest;

    QNetworkAccessManager *m_net;
    QString m_engineDirectory;
    QSet<QString> m_inFlight;
    static QString platformAssetSuffix();
    static bool isInstalled(const QString &tag, const QString &engineDirectory = {});
    void downloadArchive(const ReleaseInfo &info, const QByteArray &expectedHash);
    void handleDownloadedArchive(const QString &tag, const QString &archivePath,
                                 const std::shared_ptr<QTemporaryDir> &workDir);
    void extractArchiveMember(const QString &tag, const QString &archivePath,
                              const QString &member,
                              const std::shared_ptr<QTemporaryDir> &workDir);
    void extractMemberToFile(const QString &tag, const QString &archivePath,
                             const QString &member, const QString &targetName,
                             qint64 limit, const std::shared_ptr<QTemporaryDir> &workDir,
                             std::function<void(const QString &)> completed);
#if defined(Q_OS_WIN)
    struct WindowsInstallation;
    void extractWindowsPackage(const QString &tag, const QString &archivePath,
                               const QString &member, const QByteArray &listing,
                               const std::shared_ptr<QTemporaryDir> &workDir);
    void extractWindowsDependency(const std::shared_ptr<WindowsInstallation> &installation);
    void installWindowsDirectory(const std::shared_ptr<WindowsInstallation> &installation);
    bool installWindowsFallback(const QString &tag, const QString &binaryPath,
                                const std::shared_ptr<QTemporaryDir> &workDir);
#endif
    void installExtractedBinary(const QString &tag, const QString &binaryPath,
                                const std::shared_ptr<QTemporaryDir> &workDir);
    void fail(const QString &tag, const std::shared_ptr<QTemporaryDir> &workDir,
              const QString &error);
};
