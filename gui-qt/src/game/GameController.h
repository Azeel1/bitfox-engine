#pragma once

#include <QList>
#include <QObject>
#include <QPair>
#include <QString>
#include <QStringList>
#include <QVector>

#include "core/CoreBoard.h"

class UciEngine;

class GameController : public QObject {
    Q_OBJECT
public:
    explicit GameController(QObject *parent = nullptr);

    CoreBoard *board() { return &m_board; }
    CoreBoard *premoveBoard() { return &m_pmBoard; }
    int movetime() const { return m_movetimeMs; }
    void setMovetime(int ms) { m_movetimeMs = ms; }

    void setEngine(int color, UciEngine *engine);
    bool isHuman(int color) const { return m_engines[color] == nullptr; }

    void newGame();
    void nudge();
    void undo();
    void redo();
    QString pgn(const QString &white, const QString &black, const QString &result) const;

public slots:
    void requestHumanMove(int from, int to, int promo);
    void setPremove(int from, int to, int promo);
    void clearPremove();

signals:
    void boardChanged();
    void movePlayed(int from, int to, const QString &uci, bool engine);
    void historyRebuilt(const QStringList &moves, int lastFrom, int lastTo);
    void gameOver(int status, const QString &text);
    void statusText(const QString &text);
    void engineInfo(const QString &name, int depth, int scoreCp);
    void sound(const QString &key);
    void premovesChanged(const QVector<QPair<int, int>> &moves);

private slots:
    void onEngineBestMove(int searchId, int from, int to, int promo);
    void onEngineInfo(int depth, int scoreCp, const QString &pv);

private:
    struct Premove {
        int from;
        int to;
        int promo;
    };

    bool applyMove(int from, int to, int promo, bool engine);
    void continueGame();
    bool checkGameOver();
    QString moveToUci(int from, int to, int promo) const;
    QString sanForMove(int index) const;
    void rebuildAfterHistoryChange();
    void stopEngines();
    void emitPremoves();
    void syncPremoveBoard();

    CoreBoard m_board;
    CoreBoard m_pmBoard;
    UciEngine *m_engines[2] = {nullptr, nullptr};
    int m_movetimeMs = 1000;
    bool m_over = false;
    bool m_awaiting = false;
    int m_engineSearchIds[2] = {0, 0};
    QList<Premove> m_premoves;

    QStringList m_fens;
    QStringList m_moves;
    QStringList m_san;
    QList<QPair<int, int>> m_moveSquares;
    int m_ply = 0;
};
