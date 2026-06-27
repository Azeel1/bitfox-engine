#include "ui/MainWindow.h"

#include <QApplication>
#include <QCheckBox>
#include <QClipboard>
#include <QDialog>
#include <QHBoxLayout>
#include <QKeyEvent>
#include <QLabel>
#include <QPlainTextEdit>
#include <QPushButton>
#include <QVBoxLayout>
#include <QVector>
#include <QWidget>

#include "audio/SoundBank.h"
#include "board/BoardView.h"
#include "core/CoreBoard.h"
#include "engine/EngineCatalog.h"
#include "engine/UciEngine.h"
#include "game/GameController.h"
#include "ui/SidePanel.h"

MainWindow::MainWindow(QWidget *parent) : QMainWindow(parent) {
    setWindowTitle("Bitfox");
    resize(1040, 760);

    m_game = new GameController(this);
    m_sound = new SoundBank(this);
    m_boardView = new BoardView(this);
    m_panel = new SidePanel(this);

    auto *central = new QWidget(this);
    auto *layout = new QHBoxLayout(central);
    layout->setContentsMargins(16, 16, 16, 16);
    layout->setSpacing(16);
    layout->addWidget(m_boardView, 1);
    layout->addWidget(m_panel);
    setCentralWidget(central);

    m_boardView->setBoard(m_game->board());
    m_boardView->setPremoveBoard(m_game->premoveBoard());
    m_game->setMovetime(m_panel->movetime());

    connect(m_boardView, &BoardView::moveRequested, m_game,
            &GameController::requestHumanMove);
    connect(m_game, &GameController::boardChanged, this, [this] {
        m_boardView->refresh();
        updateCaptured();
    });
    connect(m_game, &GameController::historyRebuilt, this, &MainWindow::onHistoryRebuilt);
    connect(m_game, &GameController::statusText, m_panel, &SidePanel::setStatus);
    connect(m_game, &GameController::gameOver, this, &MainWindow::onGameOver);
    connect(m_game, &GameController::engineInfo, m_panel, &SidePanel::setEval);
    connect(m_game, &GameController::sound, this,
            [this](const QString &key) { m_sound->play(key); });

    connect(m_panel, &SidePanel::engineSelected, this, &MainWindow::onEngineSelected);
    connect(m_panel, &SidePanel::newGameRequested, this, &MainWindow::onNewGame);
    connect(m_panel, &SidePanel::flipRequested, this,
            [this] { m_boardView->setFlipped(!m_boardView->flipped()); });
    connect(m_panel, &SidePanel::movetimeChanged, m_game, &GameController::setMovetime);
    connect(m_panel, &SidePanel::undoRequested, m_game, &GameController::undo);
    connect(m_panel, &SidePanel::redoRequested, m_game, &GameController::redo);
    connect(m_panel, &SidePanel::hintRequested, this, &MainWindow::showHint);
    connect(m_panel, &SidePanel::settingsRequested, this, &MainWindow::openSettings);
    connect(m_boardView, &BoardView::premoveRequested, m_game, &GameController::setPremove);
    connect(m_game, &GameController::premovesChanged, m_boardView, &BoardView::setPremoves);

    m_panel->setEngines(EngineCatalog::discover());
    updateHumanColors();
    onNewGame();
    m_boardView->setFocus();
}

void MainWindow::keyPressEvent(QKeyEvent *event) {
    switch (event->key()) {
    case Qt::Key_U:
    case Qt::Key_Left:
    case Qt::Key_Backspace:
        m_game->undo();
        break;
    case Qt::Key_R:
    case Qt::Key_Right:
        m_game->redo();
        break;
    case Qt::Key_H:
        showHint();
        break;
    case Qt::Key_F:
        m_boardView->setFlipped(!m_boardView->flipped());
        break;
    case Qt::Key_N:
        onNewGame();
        break;
    default:
        QMainWindow::keyPressEvent(event);
        return;
    }
    event->accept();
}

void MainWindow::showHint() {
    int from = -1, to = -1, promo = 0;
    QApplication::setOverrideCursor(Qt::WaitCursor);
    bool ok = m_game->board()->bestMove(500, from, to, promo);
    QApplication::restoreOverrideCursor();
    if (ok) {
        m_boardView->setHint(from, to);
    }
}

void MainWindow::onEngineSelected(int color, const QString &path) {
    if (m_engines[color]) {
        m_game->setEngine(color, nullptr);
        m_engines[color]->deleteLater();
        m_engines[color] = nullptr;
    }
    if (!path.isEmpty()) {
        auto *engine = new UciEngine(this);
        if (engine->start(path)) {
            m_engines[color] = engine;
            m_game->setEngine(color, engine);
            connect(engine, &UciEngine::ready, m_game, &GameController::nudge);
        } else {
            engine->deleteLater();
        }
    }
    updateHumanColors();
    m_game->nudge();
}

void MainWindow::updateHumanColors() {
    m_boardView->setHumanColors(m_engines[0] == nullptr, m_engines[1] == nullptr);
}

void MainWindow::onNewGame() {
    m_boardView->clearSelection();
    m_game->newGame();
}

void MainWindow::onHistoryRebuilt(const QStringList &moves, int lastFrom, int lastTo) {
    m_panel->rebuildMoves(moves);
    m_boardView->setLastMove(lastFrom, lastTo);
    m_boardView->clearSelection();
    updateCaptured();
}

void MainWindow::onGameOver(int status, const QString &text) {
    m_panel->setStatus(text);
    QString result = "1/2-1/2";
    if (status == 1) {
        result = m_game->board()->sideToMove() == 0 ? "0-1" : "1-0";
    }
    QString white = m_engines[0] ? m_engines[0]->name() : tr("Human");
    QString black = m_engines[1] ? m_engines[1]->name() : tr("Human");
    QString pgn = m_game->pgn(white, black, result);

    QDialog dlg(this);
    dlg.setWindowTitle(tr("Game over"));
    auto *lay = new QVBoxLayout(&dlg);
    auto *title = new QLabel(QString("%1   (%2)").arg(text, result), &dlg);
    title->setStyleSheet("font-size:17px; font-weight:800; color:#81b64c;");
    lay->addWidget(title);
    auto *view = new QPlainTextEdit(&dlg);
    view->setPlainText(pgn);
    view->setReadOnly(true);
    view->setMinimumSize(380, 200);
    lay->addWidget(view);
    auto *row = new QHBoxLayout();
    auto *copy = new QPushButton(tr("Copy PGN"), &dlg);
    copy->setObjectName("primary");
    auto *close = new QPushButton(tr("Close"), &dlg);
    row->addStretch();
    row->addWidget(copy);
    row->addWidget(close);
    lay->addLayout(row);
    connect(copy, &QPushButton::clicked, &dlg,
            [pgn] { QGuiApplication::clipboard()->setText(pgn); });
    connect(close, &QPushButton::clicked, &dlg, &QDialog::accept);
    dlg.exec();
}

void MainWindow::openSettings() {
    QDialog dlg(this);
    dlg.setWindowTitle(tr("Settings"));
    auto *lay = new QVBoxLayout(&dlg);
    auto *sound = new QCheckBox(tr("Sound effects"), &dlg);
    sound->setChecked(m_soundOn);
    lay->addWidget(sound);
    auto *premove = new QCheckBox(tr("Premove"), &dlg);
    premove->setChecked(m_premoveOn);
    lay->addWidget(premove);
    auto *done = new QPushButton(tr("Done"), &dlg);
    done->setObjectName("primary");
    lay->addWidget(done);
    connect(done, &QPushButton::clicked, &dlg, &QDialog::accept);
    dlg.exec();
    m_soundOn = sound->isChecked();
    m_sound->setMuted(!m_soundOn);
    m_premoveOn = premove->isChecked();
    m_boardView->setPremoveEnabled(m_premoveOn);
    if (!m_premoveOn) {
        m_game->clearPremove();
    }
}

void MainWindow::updateCaptured() {
    CoreBoard *b = m_game->board();
    int wc[6] = {0}, bc[6] = {0};
    for (int sq = 0; sq < 64; ++sq) {
        int p = b->pieceAt(sq);
        if (p >= 0) {
            (p < 6 ? wc[p % 6] : bc[p % 6])++;
        }
    }
    const int init[6] = {8, 2, 2, 2, 1, 1};
    const int val[6] = {1, 3, 3, 5, 9, 0};

    int whiteMat = 0, blackMat = 0;
    int promoW = 0, promoB = 0;
    for (int pt = 1; pt < 5; ++pt) {
        promoW += qMax(0, wc[pt] - init[pt]);
        promoB += qMax(0, bc[pt] - init[pt]);
    }
    for (int pt = 0; pt < 5; ++pt) {
        whiteMat += wc[pt] * val[pt];
        blackMat += bc[pt] * val[pt];
    }

    QVector<int> whiteTook, blackTook;
    int blackPawnsLost = qMax(0, (init[0] - bc[0]) - promoB);
    int whitePawnsLost = qMax(0, (init[0] - wc[0]) - promoW);
    for (int k = 0; k < blackPawnsLost; ++k) whiteTook.append(6);
    for (int k = 0; k < whitePawnsLost; ++k) blackTook.append(0);
    for (int pt = 1; pt < 5; ++pt) {
        for (int k = 0; k < qMax(0, init[pt] - bc[pt]); ++k) whiteTook.append(6 + pt);
        for (int k = 0; k < qMax(0, init[pt] - wc[pt]); ++k) blackTook.append(pt);
    }

    int diff = whiteMat - blackMat;
    m_panel->setCaptured(whiteTook, diff > 0 ? diff : 0, blackTook, diff < 0 ? -diff : 0);
}
