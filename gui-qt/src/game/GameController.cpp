#include "game/GameController.h"

#include <QDate>

#include "engine/UciEngine.h"

namespace {
int pieceColor(int piece) {
    return piece < 6 ? 0 : 1;
}

bool sameColor(int a, int b) {
    return a >= 0 && b >= 0 && pieceColor(a) == pieceColor(b);
}

bool clearLine(const int pieces[64], int from, int to) {
    int ff = from & 7;
    int fr = from >> 3;
    int tf = to & 7;
    int tr = to >> 3;
    int df = tf == ff ? 0 : (tf > ff ? 1 : -1);
    int dr = tr == fr ? 0 : (tr > fr ? 1 : -1);
    for (int f = ff + df, r = fr + dr; f != tf || r != tr; f += df, r += dr) {
        if (pieces[r * 8 + f] >= 0) {
            return false;
        }
    }
    return true;
}

bool canProjectPremove(const int pieces[64], int from, int to) {
    if (from < 0 || from >= 64 || to < 0 || to >= 64 || from == to) {
        return false;
    }
    int piece = pieces[from];
    int target = pieces[to];
    if (piece < 0 || sameColor(piece, target) || (target >= 0 && target % 6 == 5)) {
        return false;
    }

    int color = pieceColor(piece);
    int type = piece % 6;
    int ff = from & 7;
    int fr = from >> 3;
    int tf = to & 7;
    int tr = to >> 3;
    int af = qAbs(tf - ff);
    int ar = qAbs(tr - fr);

    switch (type) {
    case 0: {
        int dir = color == 0 ? 1 : -1;
        int start = color == 0 ? 1 : 6;
        if (tf == ff && tr == fr + dir && target < 0) {
            return true;
        }
        if (tf == ff && fr == start && tr == fr + dir * 2 && target < 0 &&
            pieces[(fr + dir) * 8 + ff] < 0) {
            return true;
        }
        return af == 1 && tr == fr + dir;
    }
    case 1:
        return (af == 1 && ar == 2) || (af == 2 && ar == 1);
    case 2:
        return af == ar && clearLine(pieces, from, to);
    case 3:
        return (ff == tf || fr == tr) && clearLine(pieces, from, to);
    case 4:
        return ((af == ar) || ff == tf || fr == tr) && clearLine(pieces, from, to);
    case 5: {
        if (af <= 1 && ar <= 1) {
            return true;
        }
        int base = color == 0 ? 0 : 56;
        if (from != base + 4 || fr != tr || ar != 0 || af != 2) {
            return false;
        }
        if (tf == 6) {
            return pieces[base + 7] == color * 6 + 3 &&
                   pieces[base + 5] < 0 && pieces[base + 6] < 0;
        }
        if (tf == 2) {
            return pieces[base + 0] == color * 6 + 3 &&
                   pieces[base + 1] < 0 && pieces[base + 2] < 0 && pieces[base + 3] < 0;
        }
        return false;
    }
    default:
        return false;
    }
}

void applyProjectedPremove(int pieces[64], int from, int to, int promo) {
    int piece = pieces[from];
    int color = pieceColor(piece);
    pieces[from] = -1;
    if (piece % 6 == 5 && qAbs((from & 7) - (to & 7)) == 2) {
        int base = color == 0 ? 0 : 56;
        if ((to & 7) == 6) {
            pieces[base + 7] = -1;
            pieces[base + 5] = color * 6 + 3;
        } else {
            pieces[base + 0] = -1;
            pieces[base + 3] = color * 6 + 3;
        }
    }
    int toRank = to >> 3;
    if (piece % 6 == 0 && (toRank == 0 || toRank == 7) && promo >= 1 && promo <= 4) {
        pieces[to] = color * 6 + promo;
    } else {
        pieces[to] = piece;
    }
}

void removeCastlingRight(QString &castling, QChar right) {
    castling.remove(right);
}

void updateProjectedCastling(QString &castling, int from, int to) {
    switch (from) {
    case 4:
        removeCastlingRight(castling, 'K');
        removeCastlingRight(castling, 'Q');
        break;
    case 60:
        removeCastlingRight(castling, 'k');
        removeCastlingRight(castling, 'q');
        break;
    case 7:
        removeCastlingRight(castling, 'K');
        break;
    case 0:
        removeCastlingRight(castling, 'Q');
        break;
    case 63:
        removeCastlingRight(castling, 'k');
        break;
    case 56:
        removeCastlingRight(castling, 'q');
        break;
    }
    switch (to) {
    case 7:
        removeCastlingRight(castling, 'K');
        break;
    case 0:
        removeCastlingRight(castling, 'Q');
        break;
    case 63:
        removeCastlingRight(castling, 'k');
        break;
    case 56:
        removeCastlingRight(castling, 'q');
        break;
    }
}

QString fenFromPieces(const int pieces[64], const QStringList &baseParts, const QString &castling) {
    static const char glyph[] = "PNBRQKpnbrqk";
    QString fen;
    for (int rank = 7; rank >= 0; --rank) {
        int empty = 0;
        for (int file = 0; file < 8; ++file) {
            int pc = pieces[rank * 8 + file];
            if (pc < 0) {
                ++empty;
                continue;
            }
            if (empty) {
                fen += QString::number(empty);
                empty = 0;
            }
            fen += QChar(glyph[pc]);
        }
        if (empty) {
            fen += QString::number(empty);
        }
        if (rank) {
            fen += '/';
        }
    }
    QString rights = castling.isEmpty() || castling == "-" ? "-" : castling;
    QString side = baseParts.size() > 1 ? baseParts[1] : QStringLiteral("w");
    QString ep = baseParts.size() > 3 ? baseParts[3] : QStringLiteral("-");
    QString halfmove = baseParts.size() > 4 ? baseParts[4] : QStringLiteral("0");
    QString fullmove = baseParts.size() > 5 ? baseParts[5] : QStringLiteral("1");
    fen += QString(" %1 %2 %3 %4 %5").arg(side, rights, ep, halfmove, fullmove);
    return fen;
}
}

GameController::GameController(QObject *parent) : QObject(parent) {
    m_board.reset();
    m_pmBoard.setFen(m_board.fen());
}

void GameController::setEngine(int color, UciEngine *engine) {
    if (m_engines[color]) {
        disconnect(m_engines[color], nullptr, this, nullptr);
    }
    m_engines[color] = engine;
    if (engine) {
        connect(engine, &UciEngine::bestMove, this, &GameController::onEngineBestMove);
        connect(engine, &UciEngine::info, this, &GameController::onEngineInfo);
    }
}

void GameController::stopEngines() {
    m_awaiting = false;
    m_engineSearchIds[0] = 0;
    m_engineSearchIds[1] = 0;
    clearPremove();
    for (UciEngine *e : m_engines) {
        if (e) {
            e->stop();
        }
    }
}

void GameController::newGame() {
    stopEngines();
    m_board.reset();
    m_over = false;
    m_fens = {m_board.fen()};
    m_moves.clear();
    m_san.clear();
    m_moveSquares.clear();
    m_ply = 0;
    emit historyRebuilt(QStringList(), -1, -1);
    emit boardChanged();
    emit statusText(tr("New game"));
    continueGame();
}

QString GameController::moveToUci(int from, int to, int promo) const {
    QString uci = CoreBoard::squareName(from) + CoreBoard::squareName(to);
    if (promo == 1) uci += 'n';
    else if (promo == 2) uci += 'b';
    else if (promo == 3) uci += 'r';
    else if (promo == 4) uci += 'q';
    return uci;
}

QString GameController::sanForMove(int index) const {
    CoreBoard pre;
    pre.setFen(m_fens[index]);
    int from = m_moveSquares[index].first;
    int to = m_moveSquares[index].second;
    int piece = pre.pieceAt(from);
    if (piece < 0) {
        return m_moves[index];
    }
    int pt = piece % 6;
    int color = piece < 6 ? 0 : 1;
    int promo = 0;
    const QString &uci = m_moves[index];
    if (uci.size() >= 5) {
        switch (uci[4].toLatin1()) {
        case 'n': promo = 1; break;
        case 'b': promo = 2; break;
        case 'r': promo = 3; break;
        case 'q': promo = 4; break;
        }
    }
    int ff = from & 7, fr = from >> 3, tf = to & 7;

    QString san;
    if (pt == 5 && qAbs(ff - tf) == 2) {
        san = tf == 6 ? "O-O" : "O-O-O";
    } else if (pt == 0) {
        if (ff != tf) {
            san += QChar('a' + ff);
            san += 'x';
        }
        san += CoreBoard::squareName(to);
        if (promo) {
            san += '=';
            san += "NBRQ"[promo - 1];
        }
    } else {
        san += "NBRQK"[pt - 1];
        bool sameFile = false, sameRank = false, ambiguous = false;
        for (int sq = 0; sq < 64; ++sq) {
            if (sq == from) {
                continue;
            }
            int p2 = pre.pieceAt(sq);
            if (p2 < 0 || p2 % 6 != pt || (p2 < 6 ? 0 : 1) != color) {
                continue;
            }
            if (pre.legalTo(sq).contains(to)) {
                ambiguous = true;
                if ((sq & 7) == ff) sameFile = true;
                if ((sq >> 3) == fr) sameRank = true;
            }
        }
        if (ambiguous) {
            if (!sameFile) san += QChar('a' + ff);
            else if (!sameRank) san += QChar('1' + fr);
            else { san += QChar('a' + ff); san += QChar('1' + fr); }
        }
        if (pre.pieceAt(to) >= 0) {
            san += 'x';
        }
        san += CoreBoard::squareName(to);
    }

    CoreBoard post;
    post.setFen(m_fens[index + 1]);
    if (post.status() == 1) {
        san += '#';
    } else if (post.inCheck()) {
        san += '+';
    }
    return san;
}

QString GameController::pgn(const QString &white, const QString &black,
                           const QString &result) const {
    auto esc = [](QString s) {
        return s.replace('\\', "\\\\").replace('"', "\\\"").replace('\n', " ").replace('\r', " ");
    };
    QString out;
    out += "[Event \"Bitfox\"]\n";
    out += "[Site \"?\"]\n";
    out += QString("[Date \"%1\"]\n").arg(QDate::currentDate().toString("yyyy.MM.dd"));
    out += QString("[White \"%1\"]\n").arg(esc(white));
    out += QString("[Black \"%1\"]\n").arg(esc(black));
    out += QString("[Result \"%1\"]\n\n").arg(result);
    QString body;
    for (int i = 0; i < m_moves.size(); ++i) {
        if (i % 2 == 0) {
            body += QString("%1. ").arg(i / 2 + 1);
        }
        body += sanForMove(i) + " ";
    }
    body += result;
    out += body + "\n";
    return out;
}

void GameController::nudge() {
    if (!m_over) {
        continueGame();
    }
}

void GameController::syncPremoveBoard() {
    int pieces[64];
    for (int sq = 0; sq < 64; ++sq) {
        pieces[sq] = m_board.pieceAt(sq);
    }
    QStringList baseParts = m_board.fen().split(' ', Qt::SkipEmptyParts);
    QString castling = baseParts.size() > 2 ? baseParts[2] : QStringLiteral("-");
    if (castling == "-") {
        castling.clear();
    }

    QList<Premove> synced;
    for (const Premove &pm : m_premoves) {
        if (!canProjectPremove(pieces, pm.from, pm.to)) {
            break;
        }
        updateProjectedCastling(castling, pm.from, pm.to);
        synced.append(pm);
        applyProjectedPremove(pieces, pm.from, pm.to, pm.promo);
    }

    if (synced.size() != m_premoves.size()) {
        m_premoves = synced;
    }
    m_pmBoard.setFen(fenFromPieces(pieces, baseParts, castling));
}

void GameController::emitPremoves() {
    syncPremoveBoard();
    if (m_premoves.isEmpty()) {
        emit premovesChanged({});
        return;
    }
    QVector<QPair<int, int>> v;
    v.reserve(m_premoves.size());
    for (const Premove &pm : m_premoves) {
        v.append({pm.from, pm.to});
    }
    emit premovesChanged(v);
}

void GameController::setPremove(int from, int to, int promo) {
    if (from < 0) {
        clearPremove();
        return;
    }
    m_premoves.append({from, to, promo});
    emitPremoves();
}

void GameController::clearPremove() {
    if (m_premoves.isEmpty()) {
        m_pmBoard.setFen(m_board.fen());
        return;
    }
    m_premoves.clear();
    m_pmBoard.setFen(m_board.fen());
    emit premovesChanged({});
}

void GameController::undo() {
    if (m_ply == 0) {
        return;
    }
    stopEngines();
    int target = m_ply - 1;
    bool hasHuman = isHuman(0) || isHuman(1);
    if (hasHuman) {
        while (target > 0 && !isHuman(target % 2 == 0 ? 0 : 1)) {
            --target;
        }
    }
    m_ply = target;
    m_over = false;
    m_board.setFen(m_fens[m_ply]);
    rebuildAfterHistoryChange();
}

void GameController::redo() {
    if (m_ply >= m_moves.size()) {
        return;
    }
    stopEngines();
    ++m_ply;
    m_over = false;
    m_board.setFen(m_fens[m_ply]);
    emit historyRebuilt(m_san.mid(0, m_ply), m_moveSquares[m_ply - 1].first,
                        m_moveSquares[m_ply - 1].second);
    emit boardChanged();
    if (checkGameOver()) {
        return;
    }
    if (m_ply == m_moves.size()) {
        continueGame();
    } else {
        int side = m_board.sideToMove();
        emit statusText(side == 0 ? tr("White to move") : tr("Black to move"));
    }
}

void GameController::rebuildAfterHistoryChange() {
    QStringList shown = m_san.mid(0, m_ply);
    int lf = -1, lt = -1;
    if (m_ply > 0) {
        lf = m_moveSquares[m_ply - 1].first;
        lt = m_moveSquares[m_ply - 1].second;
    }
    emit historyRebuilt(shown, lf, lt);
    emit boardChanged();
    int side = m_board.sideToMove();
    emit statusText(side == 0 ? tr("White to move") : tr("Black to move"));
}

void GameController::requestHumanMove(int from, int to, int promo) {
    if (m_over || !isHuman(m_board.sideToMove())) {
        return;
    }
    applyMove(from, to, promo, false);
}

void GameController::onEngineBestMove(int searchId, int from, int to, int promo) {
    int side = m_board.sideToMove();
    if (m_over || !m_awaiting || from < 0 || isHuman(side)) {
        return;
    }
    if (sender() != m_engines[side] || searchId != m_engineSearchIds[side]) {
        return;
    }
    m_awaiting = false;
    m_engineSearchIds[side] = 0;
    if (applyMove(from, to, promo, true)) {
        return;
    }
    emit sound(QStringLiteral("illegal"));
    emit statusText(tr("%1 returned an illegal move").arg(m_engines[side]->name()));
}

void GameController::onEngineInfo(int depth, int scoreCp, const QString &) {
    if (!m_awaiting || sender() != m_engines[m_board.sideToMove()]) {
        return;
    }
    UciEngine *engine = qobject_cast<UciEngine *>(sender());
    int whiteRel = m_board.sideToMove() == 1 ? -scoreCp : scoreCp;
    emit engineInfo(engine ? engine->name() : QString(), depth, whiteRel);
}

bool GameController::applyMove(int from, int to, int promo, bool engine) {
    CcMoveInfo info;
    QString uci = moveToUci(from, to, promo);
    if (!m_board.apply(from, to, promo, &info) || !info.legal) {
        return false;
    }

    if (m_ply < m_moves.size()) {
        m_moves = m_moves.mid(0, m_ply);
        m_san = m_san.mid(0, m_ply);
        m_fens = m_fens.mid(0, m_ply + 1);
        m_moveSquares = m_moveSquares.mid(0, m_ply);
    }
    m_moves.append(uci);
    m_fens.append(m_board.fen());
    m_moveSquares.append({from, to});
    ++m_ply;
    m_san.append(sanForMove(m_ply - 1));
    if (!m_premoves.isEmpty()) {
        emitPremoves();
    } else {
        m_pmBoard.setFen(m_board.fen());
    }

    QString snd = "move";
    if (info.status == 1 || info.status >= 2) snd = "end";
    else if (info.check) snd = "check";
    else if (info.castle) snd = "castle";
    else if (info.promo) snd = "promote";
    else if (info.capture || info.ep) snd = "capture";
    emit sound(snd);

    emit movePlayed(from, to, uci, engine);
    emit historyRebuilt(m_san.mid(0, m_ply), from, to);
    emit boardChanged();
    if (checkGameOver()) {
        return true;
    }
    continueGame();
    return true;
}

bool GameController::checkGameOver() {
    int status = m_board.status();
    if (status == 0) {
        return false;
    }
    m_over = true;
    clearPremove();
    QString text;
    switch (status) {
    case 1: text = m_board.sideToMove() == 0 ? tr("Checkmate - Black wins")
                                             : tr("Checkmate - White wins"); break;
    case 2: text = tr("Stalemate - draw"); break;
    case 3: text = tr("Draw - fifty-move rule"); break;
    case 4: text = tr("Draw - threefold repetition"); break;
    case 5: text = tr("Draw - insufficient material"); break;
    default: text = tr("Game over"); break;
    }
    emit gameOver(status, text);
    emit statusText(text);
    return true;
}

void GameController::continueGame() {
    int side = m_board.sideToMove();
    if (isHuman(side)) {
        if (!m_premoves.isEmpty()) {
            Premove pm = m_premoves.first();
            if (m_board.legalTo(pm.from).contains(pm.to)) {
                m_premoves.removeFirst();
                applyMove(pm.from, pm.to, pm.promo, false);
                return;
            }
            clearPremove();
            emit sound("illegal");
        }
        emit statusText(side == 0 ? tr("White to move") : tr("Black to move"));
        return;
    }
    if (!m_premoves.isEmpty()) {
        emitPremoves();
    }
    if (!m_engines[side]->isReady()) {
        emit statusText(tr("%1 is starting...").arg(m_engines[side]->name()));
        return;
    }
    emit statusText(tr("%1 is thinking...").arg(m_engines[side]->name()));
    int searchId = m_engines[side]->play(m_board.fen(), m_movetimeMs);
    if (searchId == 0) {
        emit statusText(tr("%1 is starting...").arg(m_engines[side]->name()));
        return;
    }
    m_engineSearchIds[side] = searchId;
    m_awaiting = true;
}
