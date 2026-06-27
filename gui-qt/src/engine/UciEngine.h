#pragma once

#include <QObject>
#include <QProcess>
#include <QString>

class UciEngine : public QObject {
    Q_OBJECT
public:
    explicit UciEngine(QObject *parent = nullptr);
    ~UciEngine() override;

    bool start(const QString &path);
    void shutdown();
    bool isRunning() const;
    bool isReady() const { return m_ready; }
    QString name() const { return m_name; }

    int play(const QString &fen, int movetimeMs);
    void stop();

signals:
    void ready();
    void bestMove(int searchId, int from, int to, int promo);
    void info(int depth, int scoreCp, const QString &pv);
    void nameResolved(const QString &name);

private slots:
    void onReadyRead();

private:
    void send(const QString &command);
    void startSearch(int searchId, const QString &fen, int movetimeMs);
    void startPendingSearch();
    void handleLine(const QString &line);

    QProcess m_proc;
    QString m_name = QStringLiteral("Engine");
    QByteArray m_buffer;
    QString m_pendingFen;
    int m_pendingMovetimeMs = 0;
    int m_pendingSearchId = 0;
    int m_activeSearchId = 0;
    int m_nextSearchId = 0;
    bool m_searching = false;
    bool m_stopping = false;
    bool m_ready = false;
    bool m_uciOk = false;
};
