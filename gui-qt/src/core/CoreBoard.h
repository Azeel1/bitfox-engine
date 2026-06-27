#pragma once

#include <QString>
#include <QVector>

#include "bitfox_core.h"

class CoreBoard {
public:
    CoreBoard();
    ~CoreBoard();
    CoreBoard(const CoreBoard &) = delete;
    CoreBoard &operator=(const CoreBoard &) = delete;

    void reset();
    bool setFen(const QString &fen);
    QString fen() const;

    int pieceAt(int square) const;
    QVector<int> legalTo(int from) const;
    QVector<int> premoveTargets(int from) const;
    bool apply(int from, int to, int promo, CcMoveInfo *info);

    int sideToMove() const;
    bool inCheck() const;
    int status() const;
    int evaluate() const;
    bool bestMove(int movetimeMs, int &from, int &to, int &promo) const;

    static QString squareName(int square);
    static int squareFromName(const QString &name);

private:
    CcBoard *m_board;
};
