#include "core/CoreBoard.h"

static const char *START_FEN =
    "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1";

CoreBoard::CoreBoard() : m_board(cc_new()) {}

CoreBoard::~CoreBoard() { cc_free(m_board); }

void CoreBoard::reset() { cc_set_fen(m_board, START_FEN); }

bool CoreBoard::setFen(const QString &fen) {
    return cc_set_fen(m_board, fen.toUtf8().constData()) != 0;
}

QString CoreBoard::fen() const {
    char buf[128];
    if (cc_get_fen(m_board, buf, sizeof(buf))) {
        return QString::fromUtf8(buf);
    }
    return QString();
}

int CoreBoard::pieceAt(int square) const { return cc_piece_at(m_board, square); }

QVector<int> CoreBoard::legalTo(int from) const {
    int out[32];
    int n = cc_legal_to(m_board, from, out);
    QVector<int> moves;
    moves.reserve(n);
    for (int i = 0; i < n; ++i) {
        moves.append(out[i]);
    }
    return moves;
}

QVector<int> CoreBoard::premoveTargets(int from) const {
    int out[32];
    int n = cc_premove_to(m_board, from, out);
    QVector<int> moves;
    moves.reserve(n);
    for (int i = 0; i < n; ++i) {
        moves.append(out[i]);
    }
    return moves;
}

bool CoreBoard::apply(int from, int to, int promo, CcMoveInfo *info) {
    return cc_apply(m_board, from, to, promo, info) != 0;
}

int CoreBoard::sideToMove() const { return cc_side(m_board); }

bool CoreBoard::inCheck() const { return cc_in_check(m_board) != 0; }

int CoreBoard::status() const { return cc_status(m_board); }

int CoreBoard::evaluate() const { return cc_evaluate(m_board); }

bool CoreBoard::bestMove(int movetimeMs, int &from, int &to, int &promo) const {
    CcSearchResult r;
    cc_search(m_board, 0, movetimeMs, &r);
    from = r.best & 0x3f;
    to = (r.best >> 6) & 0x3f;
    promo = (r.best >> 12) & 0x7;
    return from != to;
}

QString CoreBoard::squareName(int square) {
    if (square < 0 || square > 63) {
        return QString();
    }
    QChar file = QChar('a' + (square & 7));
    QChar rank = QChar('1' + (square >> 3));
    return QString(file) + QString(rank);
}

int CoreBoard::squareFromName(const QString &name) {
    if (name.size() < 2) {
        return -1;
    }
    int file = name[0].toLatin1() - 'a';
    int rank = name[1].toLatin1() - '1';
    if (file < 0 || file > 7 || rank < 0 || rank > 7) {
        return -1;
    }
    return rank * 8 + file;
}
