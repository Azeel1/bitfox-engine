#pragma once

#include <QObject>
#include <QPointer>
#include <QProcess>
#include <QString>
#include <QStringList>
#include <QTimer>

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

    int play(const QString &fen, int movetimeMs, const QStringList &moves = {});
    void newGame();
    void stop();
    void rejectMove();

signals:
    void ready();
    void bestMove(int searchId, int from, int to, int promo);
    void nameResolved(const QString &name);
    void failed(const QString &error);

private slots:
    void onReadyRead();
    void onProcessError(QProcess::ProcessError error);
    void onProcessFinished(int exitCode, QProcess::ExitStatus status);

private:
    bool send(const QString &command);
    void createProcess();
    void abandonProcess();
    bool stopProcessBounded();
    bool startSearch(int searchId, const QString &fen, int movetimeMs,
                     const QStringList &moves);
    void startPendingSearch();
    void processBufferedOutput();
    void rejectOversizedOutput();
    void handleLine(const QString &line);
    void resetProtocolState();
    void reportFailure(const QString &error);

    QProcess *m_proc = nullptr;
    QPointer<QProcess> m_abandonedProcess;
    QTimer m_handshakeTimer;
    QTimer m_searchTimer;
    QString m_name = QStringLiteral("Engine");
    QByteArray m_buffer;
    QString m_pendingFen;
    QStringList m_pendingMoves;
    int m_pendingMovetimeMs = 0;
    int m_pendingSearchId = 0;
    int m_activeSearchId = 0;
    int m_nextSearchId = 0;
    bool m_searching = false;
    bool m_stopping = false;
    bool m_ready = false;
    bool m_uciOk = false;
    bool m_starting = false;
    bool m_startFailed = false;
    bool m_outputReadQueued = false;
    bool m_shuttingDown = false;
    bool m_failureReported = false;
    bool m_newGamePending = false;
};
