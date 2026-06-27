#include "engine/UciEngine.h"

#include <QFileInfo>

#include "core/CoreBoard.h"

UciEngine::UciEngine(QObject *parent) : QObject(parent) {
    connect(&m_proc, &QProcess::readyReadStandardOutput, this, &UciEngine::onReadyRead);
}

UciEngine::~UciEngine() { shutdown(); }

bool UciEngine::start(const QString &path) {
    shutdown();
    m_name = QFileInfo(path).fileName();
    m_buffer.clear();
    m_pendingFen.clear();
    m_pendingMovetimeMs = 0;
    m_pendingSearchId = 0;
    m_activeSearchId = 0;
    m_nextSearchId = 0;
    m_searching = false;
    m_stopping = false;
    m_ready = false;
    m_uciOk = false;
    m_proc.start(path, QStringList());
    if (!m_proc.waitForStarted(3000)) {
        return false;
    }
    send(QStringLiteral("uci"));
    return true;
}

void UciEngine::shutdown() {
    if (m_proc.state() != QProcess::NotRunning) {
        send(QStringLiteral("quit"));
        if (!m_proc.waitForFinished(800)) {
            m_proc.kill();
            m_proc.waitForFinished(400);
        }
    }
    m_buffer.clear();
    m_pendingFen.clear();
    m_pendingMovetimeMs = 0;
    m_pendingSearchId = 0;
    m_activeSearchId = 0;
    m_searching = false;
    m_stopping = false;
    m_ready = false;
    m_uciOk = false;
}

bool UciEngine::isRunning() const { return m_proc.state() == QProcess::Running; }

void UciEngine::send(const QString &command) {
    if (m_proc.state() != QProcess::NotRunning) {
        m_proc.write(command.toUtf8() + '\n');
    }
}

int UciEngine::play(const QString &fen, int movetimeMs) {
    if (!m_ready) {
        return 0;
    }
    int searchId = ++m_nextSearchId;
    if (m_searching) {
        m_pendingFen = fen;
        m_pendingMovetimeMs = movetimeMs;
        m_pendingSearchId = searchId;
        m_stopping = true;
        send(QStringLiteral("stop"));
        return searchId;
    }
    if (m_stopping) {
        m_pendingFen = fen;
        m_pendingMovetimeMs = movetimeMs;
        m_pendingSearchId = searchId;
        return searchId;
    }
    startSearch(searchId, fen, movetimeMs);
    return searchId;
}

void UciEngine::startSearch(int searchId, const QString &fen, int movetimeMs) {
    m_activeSearchId = searchId;
    send(QStringLiteral("position fen %1").arg(fen));
    send(QStringLiteral("go movetime %1").arg(movetimeMs));
    m_searching = true;
}

void UciEngine::startPendingSearch() {
    if (m_pendingSearchId == 0) {
        return;
    }
    const QString fen = m_pendingFen;
    int movetimeMs = m_pendingMovetimeMs;
    int searchId = m_pendingSearchId;
    m_pendingFen.clear();
    m_pendingMovetimeMs = 0;
    m_pendingSearchId = 0;
    startSearch(searchId, fen, movetimeMs);
}

void UciEngine::stop() {
    if (m_searching) {
        m_stopping = true;
        send(QStringLiteral("stop"));
    }
    m_pendingFen.clear();
    m_pendingMovetimeMs = 0;
    m_pendingSearchId = 0;
}

void UciEngine::onReadyRead() {
    m_buffer += m_proc.readAllStandardOutput();
    int nl;
    while ((nl = m_buffer.indexOf('\n')) >= 0) {
        QString line = QString::fromUtf8(m_buffer.left(nl)).trimmed();
        m_buffer.remove(0, nl + 1);
        if (!line.isEmpty()) {
            handleLine(line);
        }
    }
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
        m_uciOk = true;
        send(QStringLiteral("isready"));
        return;
    }
    if (tokens.first() == QLatin1String("readyok")) {
        if (!m_uciOk) {
            return;
        }
        m_ready = true;
        emit ready();
        return;
    }
    if (tokens.first() == QLatin1String("info")) {
        int depth = 0;
        int score = 0;
        QString pv;
        for (int i = 1; i < tokens.size(); ++i) {
            if (tokens[i] == QLatin1String("depth") && i + 1 < tokens.size()) {
                depth = tokens[i + 1].toInt();
            } else if (tokens[i] == QLatin1String("score") && i + 2 < tokens.size()) {
                if (tokens[i + 1] == QLatin1String("cp")) {
                    score = tokens[i + 2].toInt();
                } else if (tokens[i + 1] == QLatin1String("mate")) {
                    int m = tokens[i + 2].toInt();
                    score = m > 0 ? 100000 : -100000;
                }
            } else if (tokens[i] == QLatin1String("pv")) {
                pv = tokens.mid(i + 1).join(' ');
                break;
            }
        }
        emit info(depth, score, pv);
        return;
    }
    if (tokens.first() == QLatin1String("bestmove") && tokens.size() >= 2) {
        int searchId = m_activeSearchId;
        bool suppress = m_stopping || searchId == 0;
        m_searching = false;
        m_activeSearchId = 0;
        if (m_stopping) {
            m_stopping = false;
            startPendingSearch();
            return;
        }
        if (suppress) {
            return;
        }
        const QString uci = tokens[1];
        if (uci.size() >= 4) {
            int from = CoreBoard::squareFromName(uci.left(2));
            int to = CoreBoard::squareFromName(uci.mid(2, 2));
            if (from < 0 || to < 0) {
                return;
            }
            int promo = 0;
            if (uci.size() >= 5) {
                switch (uci[4].toLatin1()) {
                case 'n': promo = 1; break;
                case 'b': promo = 2; break;
                case 'r': promo = 3; break;
                case 'q': promo = 4; break;
                default: promo = 0; break;
                }
            }
            emit bestMove(searchId, from, to, promo);
        }
    }
}
