#include "engine/UciEngine.h"

#include <QFileInfo>

#include <limits>

#include "core/CoreBoard.h"

namespace {
constexpr int kStartTimeoutMs = 3000;
constexpr int kQuitTimeoutMs = 800;
constexpr int kKillTimeoutMs = 400;
constexpr qint64 kOutputChunkBytes = 16 * 1024;
constexpr qsizetype kMaxUciLineBytes = 64 * 1024;
constexpr qsizetype kMaxBufferedOutputBytes = 256 * 1024;
}

UciEngine::UciEngine(QObject *parent) : QObject(parent) {
    createProcess();
    m_handshakeTimer.setSingleShot(true);
    connect(&m_handshakeTimer, &QTimer::timeout, this,
            [this] { reportFailure(tr("UCI handshake timed out")); });
    m_searchTimer.setSingleShot(true);
    connect(&m_searchTimer, &QTimer::timeout, this,
            [this] { reportFailure(tr("engine search timed out")); });
}

UciEngine::~UciEngine() { shutdown(); }

void UciEngine::createProcess() {
    m_proc = new QProcess(this);
    m_proc->setReadChannel(QProcess::StandardOutput);
    m_proc->setStandardErrorFile(QProcess::nullDevice());
    connect(m_proc, &QProcess::readyReadStandardOutput, this, &UciEngine::onReadyRead);
    connect(m_proc, &QProcess::errorOccurred, this, &UciEngine::onProcessError);
    connect(m_proc, QOverload<int, QProcess::ExitStatus>::of(&QProcess::finished), this,
            &UciEngine::onProcessFinished);
}

void UciEngine::abandonProcess() {
    QProcess *proc = m_proc;
    m_proc = nullptr;
    QObject::disconnect(proc, nullptr, this, nullptr);
    proc->setParent(nullptr);
    m_abandonedProcess = proc;
    connect(proc, &QProcess::readyReadStandardOutput, proc, [proc] {
        while (proc->bytesAvailable() > 0) {
            proc->read(kOutputChunkBytes);
        }
    });
    connect(proc, QOverload<int, QProcess::ExitStatus>::of(&QProcess::finished), proc,
            &QObject::deleteLater);
}

bool UciEngine::start(const QString &path) {
    shutdown();
    if (m_abandonedProcess) {
        m_failureReported = true;
        return false;
    }
    if (!m_proc) {
        createProcess();
    }
    if (m_proc->state() != QProcess::NotRunning) {
        m_failureReported = true;
        return false;
    }

    m_name = QFileInfo(path).fileName();
    m_nextSearchId = 0;
    m_failureReported = false;
    m_shuttingDown = false;
    resetProtocolState();
    m_starting = true;

    const auto failStart = [this] {
        m_starting = false;
        m_shuttingDown = true;
        const bool stopped = stopProcessBounded();
        if (stopped && m_proc) {
            m_proc->close();
        }
        resetProtocolState();
        m_failureReported = true;
        m_shuttingDown = !stopped;
        return false;
    };

    m_proc->start(path, QStringList());
    if (!m_proc->waitForStarted(kStartTimeoutMs) ||
        m_proc->state() != QProcess::Running || m_startFailed) {
        return failStart();
    }
    if (!send(QStringLiteral("uci")) || !m_proc ||
        m_proc->state() != QProcess::Running || m_startFailed) {
        return failStart();
    }
    m_handshakeTimer.start(5000);
    m_starting = false;
    if (!m_buffer.isEmpty()) {
        QTimer::singleShot(0, this, [this] {
            if (!m_starting && !m_shuttingDown && !m_failureReported) {
                processBufferedOutput();
            }
        });
    }
    return true;
}

void UciEngine::shutdown() {
    m_shuttingDown = true;
    m_starting = false;
    const bool stopped = stopProcessBounded();
    if (stopped && m_proc) {
        m_proc->close();
    }
    resetProtocolState();
    m_shuttingDown = !stopped;
}

bool UciEngine::stopProcessBounded() {
    if (!m_proc || m_proc->state() == QProcess::NotRunning) {
        return true;
    }
    if (m_proc->state() == QProcess::Running) {
        (void)send(QStringLiteral("quit"));
        m_proc->closeWriteChannel();
    }
    if (m_proc->waitForFinished(kQuitTimeoutMs) ||
        m_proc->state() == QProcess::NotRunning) {
        return true;
    }
    m_proc->kill();
    if (m_proc->waitForFinished(kKillTimeoutMs) ||
        m_proc->state() == QProcess::NotRunning) {
        return true;
    }
    abandonProcess();
    return false;
}

void UciEngine::resetProtocolState() {
    m_handshakeTimer.stop();
    m_searchTimer.stop();
    m_buffer.clear();
    m_pendingFen.clear();
    m_pendingMoves.clear();
    m_pendingMovetimeMs = 0;
    m_pendingSearchId = 0;
    m_activeSearchId = 0;
    m_searching = false;
    m_stopping = false;
    m_ready = false;
    m_uciOk = false;
    m_starting = false;
    m_startFailed = false;
    m_outputReadQueued = false;
    m_newGamePending = false;
}

bool UciEngine::isRunning() const {
    return m_proc && m_proc->state() == QProcess::Running;
}

bool UciEngine::send(const QString &command) {
    if (!m_proc || m_proc->state() != QProcess::Running) {
        return false;
    }
    const QByteArray bytes = command.toUtf8() + '\n';
    return m_proc->write(bytes) == bytes.size();
}

int UciEngine::play(const QString &fen, int movetimeMs, const QStringList &moves) {
    if (!m_ready || !isRunning() || movetimeMs <= 0 || fen.isEmpty() ||
        fen.contains('\n') || fen.contains('\r')) {
        return 0;
    }
    for (const QString &move : moves) {
        if ((move.size() != 4 && move.size() != 5) ||
            CoreBoard::squareFromName(move.left(2)) < 0 ||
            CoreBoard::squareFromName(move.mid(2, 2)) < 0 ||
            (move.size() == 5 && !QStringLiteral("nbrq").contains(move[4]))) {
            return 0;
        }
    }
    if (m_nextSearchId == std::numeric_limits<int>::max()) {
        m_nextSearchId = 0;
    }
    const int searchId = ++m_nextSearchId;
    if (m_stopping) {
        m_pendingFen = fen;
        m_pendingMoves = moves;
        m_pendingMovetimeMs = movetimeMs;
        m_pendingSearchId = searchId;
        return searchId;
    }
    if (m_searching) {
        m_pendingFen = fen;
        m_pendingMoves = moves;
        m_pendingMovetimeMs = movetimeMs;
        m_pendingSearchId = searchId;
        m_stopping = true;
        if (!send(QStringLiteral("stop"))) {
            reportFailure(tr("could not stop the engine search"));
            return 0;
        }
        m_searchTimer.start(5000);
        return searchId;
    }
    return startSearch(searchId, fen, movetimeMs, moves) ? searchId : 0;
}

bool UciEngine::startSearch(int searchId, const QString &fen, int movetimeMs,
                           const QStringList &moves) {
    m_activeSearchId = searchId;
    m_searching = true;
    const qint64 requestedTimeout =
        qMax<qint64>(10000, static_cast<qint64>(movetimeMs) + 5000);
    m_searchTimer.start(static_cast<int>(qMin<qint64>(
        requestedTimeout, std::numeric_limits<int>::max())));
    QString position = QStringLiteral("position fen %1").arg(fen);
    if (!moves.isEmpty()) {
        position += QStringLiteral(" moves ") + moves.join(' ');
    }
    if (m_newGamePending) {
        m_newGamePending = false;
        if (!send(QStringLiteral("ucinewgame"))) {
            reportFailure(tr("could not start the engine search"));
            return false;
        }
    }
    if (!send(position) || !send(QStringLiteral("go movetime %1").arg(movetimeMs))) {
        reportFailure(tr("could not start the engine search"));
        return false;
    }
    return true;
}

void UciEngine::startPendingSearch() {
    if (m_pendingSearchId == 0) {
        return;
    }
    const QString fen = m_pendingFen;
    const QStringList moves = m_pendingMoves;
    int movetimeMs = m_pendingMovetimeMs;
    int searchId = m_pendingSearchId;
    m_pendingFen.clear();
    m_pendingMoves.clear();
    m_pendingMovetimeMs = 0;
    m_pendingSearchId = 0;
    (void)startSearch(searchId, fen, movetimeMs, moves);
}

void UciEngine::newGame() {
    m_newGamePending = true;
}

void UciEngine::stop() {
    if (m_searching) {
        m_stopping = true;
        if (!send(QStringLiteral("stop"))) {
            reportFailure(tr("could not stop the engine search"));
            return;
        }
        m_searchTimer.start(5000);
    }
    m_pendingFen.clear();
    m_pendingMoves.clear();
    m_pendingMovetimeMs = 0;
    m_pendingSearchId = 0;
}

void UciEngine::onReadyRead() {
    if (!m_proc) {
        return;
    }
    QPointer<UciEngine> guard(this);
    QPointer<QProcess> proc(m_proc);
    qint64 remaining = kMaxBufferedOutputBytes;
    while (remaining > 0) {
        const QByteArray chunk = proc->read(kOutputChunkBytes);
        if (chunk.isEmpty()) {
            break;
        }
        remaining -= chunk.size();
        if (m_shuttingDown || m_failureReported || m_startFailed) {
            m_buffer.clear();
            continue;
        }
        if (m_buffer.size() > kMaxBufferedOutputBytes - chunk.size()) {
            rejectOversizedOutput();
            if (!guard || !proc || m_proc != proc.data()) {
                return;
            }
            continue;
        }
        m_buffer += chunk;
        if (!m_starting) {
            processBufferedOutput();
            if (!guard || !proc || m_proc != proc.data()) {
                return;
            }
        }
    }
    if (!guard || !proc || m_proc != proc.data()) {
        return;
    }
    if (proc->bytesAvailable() > 0 && !m_outputReadQueued) {
        m_outputReadQueued = true;
        QTimer::singleShot(0, this, [this] {
            m_outputReadQueued = false;
            onReadyRead();
        });
    }
}

void UciEngine::processBufferedOutput() {
    if (m_starting || m_shuttingDown || m_failureReported) {
        return;
    }
    QPointer<UciEngine> guard(this);
    for (;;) {
        const qsizetype nl = m_buffer.indexOf('\n');
        if (nl < 0) {
            break;
        }
        if (nl > kMaxUciLineBytes) {
            rejectOversizedOutput();
            return;
        }
        QString line = QString::fromUtf8(m_buffer.left(nl)).trimmed();
        m_buffer.remove(0, nl + 1);
        if (!line.isEmpty()) {
            handleLine(line);
            if (!guard) {
                return;
            }
        }
        if (m_shuttingDown || m_failureReported) {
            m_buffer.clear();
            return;
        }
    }
    if (m_buffer.size() > kMaxUciLineBytes) {
        rejectOversizedOutput();
    }
}

void UciEngine::rejectOversizedOutput() {
    m_buffer.clear();
    if (m_starting) {
        m_startFailed = true;
        if (m_proc && m_proc->state() != QProcess::NotRunning) {
            m_proc->kill();
        }
        return;
    }
    reportFailure(tr("engine output exceeded the line limit"));
}

void UciEngine::onProcessError(QProcess::ProcessError error) {
    if (m_starting) {
        m_startFailed = true;
        return;
    }
    if (m_shuttingDown || m_failureReported || error == QProcess::FailedToStart) {
        return;
    }
    reportFailure(m_proc ? m_proc->errorString() : tr("engine process failed"));
}

void UciEngine::onProcessFinished(int, QProcess::ExitStatus status) {
    if (m_starting) {
        m_startFailed = true;
        return;
    }
    if (m_shuttingDown) {
        resetProtocolState();
        m_shuttingDown = false;
        return;
    }
    if (m_failureReported) {
        return;
    }
    reportFailure(status == QProcess::CrashExit ? tr("engine process crashed")
                                                 : tr("engine process exited"));
}

void UciEngine::reportFailure(const QString &error) {
    if (m_failureReported || m_shuttingDown) {
        return;
    }
    m_failureReported = true;
    m_starting = false;
    resetProtocolState();
    if (m_proc && m_proc->state() != QProcess::NotRunning) {
        m_proc->kill();
    }
    emit failed(error);
}

void UciEngine::rejectMove() {
    reportFailure(tr("engine returned an illegal move"));
}

void UciEngine::handleLine(const QString &line) {
    const QStringList tokens = line.split(' ', Qt::SkipEmptyParts);
    if (tokens.isEmpty()) {
        return;
    }

    if (tokens.first() == QLatin1String("id") && tokens.size() >= 3 &&
        tokens[1] == QLatin1String("name")) {
        m_name = tokens.mid(2).join(' ');
        emit nameResolved(m_name);
        return;
    }
    if (tokens.first() == QLatin1String("uciok")) {
        if (m_uciOk || m_ready) {
            return;
        }
        m_uciOk = true;
        if (!send(QStringLiteral("isready"))) {
            reportFailure(tr("could not complete the UCI handshake"));
        }
        return;
    }
    if (tokens.first() == QLatin1String("readyok")) {
        if (!m_uciOk || m_ready) {
            return;
        }
        m_handshakeTimer.stop();
        m_ready = true;
        emit ready();
        return;
    }
    if (tokens.first() == QLatin1String("bestmove")) {
        if (!m_searching || m_activeSearchId == 0) {
            return;
        }
        m_searchTimer.stop();
        int searchId = m_activeSearchId;
        m_searching = false;
        m_activeSearchId = 0;
        if (m_stopping) {
            m_stopping = false;
            startPendingSearch();
            return;
        }
        if (tokens.size() < 2 || tokens[1] == QLatin1String("(none)") ||
            tokens[1] == QLatin1String("0000")) {
            reportFailure(tr("engine returned no move"));
            return;
        }
        const QString uci = tokens[1];
        if (uci.size() == 4 || uci.size() == 5) {
            int from = CoreBoard::squareFromName(uci.left(2));
            int to = CoreBoard::squareFromName(uci.mid(2, 2));
            if (from < 0 || to < 0) {
                reportFailure(tr("engine returned an invalid move"));
                return;
            }
            int promo = 0;
            if (uci.size() == 5) {
                switch (uci[4].toLower().toLatin1()) {
                case 'n': promo = 1; break;
                case 'b': promo = 2; break;
                case 'r': promo = 3; break;
                case 'q': promo = 4; break;
                default:
                    reportFailure(tr("engine returned an invalid move"));
                    return;
                }
            }
            emit bestMove(searchId, from, to, promo);
        } else {
            reportFailure(tr("engine returned an invalid move"));
        }
    }
}
