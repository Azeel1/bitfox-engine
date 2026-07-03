#pragma once

#include <QObject>
#include <QSet>
#include <QString>
#include <QVector>

class QNetworkAccessManager;
class QNetworkReply;

struct ReleaseInfo {
    QString tag;
    QString name;
    QString assetUrl;
    QString assetName;
    bool installed = false;
};

class ReleaseManager : public QObject {
    Q_OBJECT
public:
    explicit ReleaseManager(QObject *parent = nullptr);

    void fetchReleases();
    void downloadRelease(const ReleaseInfo &info);
    static QString enginesDir();

signals:
    void releasesReady(const QVector<ReleaseInfo> &releases);
    void fetchFailed(const QString &error);
    void downloadProgress(const QString &tag, qint64 received, qint64 total);
    void downloadFinished(const QString &tag, bool success, const QString &error);

private:
    QNetworkAccessManager *m_net;
    QSet<QString> m_inFlight;
    static QString platformAssetSuffix();
    static bool isInstalled(const QString &tag);
    void handleDownloadedArchive(const QString &tag, const QString &archivePath);
    void installExtractedBinary(const QString &tag, const QString &archiveDir,
                                 const QString &extractDir);
    void fail(const QString &tag, const QString &archiveDir, const QString &error);
};
