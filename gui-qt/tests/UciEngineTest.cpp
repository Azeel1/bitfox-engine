#include <QCoreApplication>
#include <QApplication>
#include <QElapsedTimer>
#include <QPointer>
#include <QSignalSpy>
#include <QtTest>

#include <chrono>
#include <iostream>
#include <string>
#include <thread>
#include <vector>

#include "engine/UciEngine.h"
#include "game/GameController.h"
#include "board/BoardView.h"

namespace {
const char *kFen = "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1";

void writeLine(const std::string &line) {
    std::cout << line << '\n' << std::flush;
}

int runActiveNewGameEngine() {
    const std::string position = std::string("position fen ") + kFen;
    const std::vector<std::string> expected = {
        "uci", "isready", "ucinewgame", position, "go movetime 1000",
        "stop", "ucinewgame", position, "go movetime 1000"};
    std::size_t step = 0;
    std::string line;
    while (std::getline(std::cin, line)) {
        if (line == "quit") {
            return 0;
        }
        if (step >= expected.size() || line != expected[step]) {
            return 1;
        }
        if (step == 0) {
            writeLine("id name Bitfox GUI test engine");
            writeLine("uciok");
        } else if (step == 1) {
            writeLine("readyok");
        } else if (step == 4) {
            writeLine("id name active-newgame-search");
        } else if (step == 5) {
            writeLine("bestmove e2e4");
        } else if (step == 8) {
            writeLine("bestmove d2d4");
        }
        ++step;
    }
    return 0;
}

int runFakeEngine(const std::string &mode) {
    if (mode == "newgame-active") {
        return runActiveNewGameEngine();
    }
    int searches = 0;
    int newGames = 0;
    bool positionBeforeNewGame = false;
    std::string line;
    std::string position;
    const std::string expectedPosition = qgetenv("BITFOX_UCI_TEST_POSITION").toStdString();
    while (std::getline(std::cin, line)) {
        if (line == "uci") {
            if (mode == "oversized") {
                writeLine(std::string(65537, 'x'));
                continue;
            }
            writeLine("id name Bitfox GUI test engine");
            writeLine("uciok");
        } else if (line == "isready") {
            writeLine("readyok");
            if (mode == "duplicate-ready") {
                writeLine("readyok");
            }
        } else if (line == "ucinewgame") {
            ++newGames;
        } else if (line.rfind("position ", 0) == 0) {
            positionBeforeNewGame = positionBeforeNewGame || newGames == 0;
            position = line;
        } else if (line.rfind("go movetime ", 0) == 0) {
            ++searches;
            if ((mode == "replace" || mode == "replace-history") && searches == 1) {
                continue;
            }
            if (mode == "invalid" ||
                (mode == "newgame" && (newGames != 1 || positionBeforeNewGame)) ||
                ((mode == "history" || mode == "replace-history") &&
                 position != expectedPosition)) {
                writeLine("bestmove z9z9");
            } else if (mode == "exit") {
                return 0;
            } else if (mode == "illegal") {
                writeLine("bestmove e2e5");
            } else if (mode == "promotion") {
                writeLine("bestmove a7a8Q");
            } else if (mode == "newgame") {
                writeLine(searches == 1 ? "bestmove e2e4" : "bestmove g1f3");
            } else if (mode == "opening") {
                writeLine("bestmove e2e4");
            } else {
                writeLine(searches == 1 ? "bestmove e2e4" : "bestmove d7d5");
            }
        } else if (line == "stop") {
            writeLine("bestmove e2e4");
        } else if (line == "quit") {
            if (mode == "stubborn") {
                for (;;) {
                    std::this_thread::sleep_for(std::chrono::seconds(1));
                }
            }
            return 0;
        }
    }
    if (mode == "stubborn") {
        for (;;) {
            std::this_thread::sleep_for(std::chrono::seconds(1));
        }
    }
    return 0;
}

bool startFake(UciEngine *engine, const char *mode) {
    qputenv("BITFOX_UCI_TEST_MODE", mode);
    const bool started = engine->start(QCoreApplication::applicationFilePath());
    qunsetenv("BITFOX_UCI_TEST_MODE");
    return started;
}
}

class UciEngineTest : public QObject {
    Q_OBJECT

private slots:
    void handshakeAndMove();
    void duplicateReadyIsIgnored();
    void newestPendingSearchWins();
    void malformedMoveFailsOnce();
    void oversizedOutputFailsOnce();
    void invalidRequestsAreRejected();
    void processExitFailsOnce();
    void shutdownIsBounded();
    void synchronousReadyDeletionIsSafe();
    void historySurvivesUndoRedo();
    void engineReceivesGameHistory();
    void queuedHistoryIsPreserved();
    void newGameResetsTheEngine();
    void newGameDuringSearchDiscardsTheStoppedMove();
    void undoingTheEnginesFirstMoveResumesTheEngine();
    void illegalMoveReleasesTheSeat();
    void promotionSuffixIsCaseInsensitive();
    void pgnFollowsHistoryCursor();
    void capturesSurvivePromotionUndoAndBranch();
    void enPassantProjectionRemovesCapturedPawn();
    void premoveProjectionCanAwaitAnUnpin();
    void cancelledPromotionPreservesNextAnimation();
    void synchronousWriteFailureReleasesSeat();
};

void UciEngineTest::handshakeAndMove() {
    UciEngine engine;
    QSignalSpy ready(&engine, &UciEngine::ready);
    QSignalSpy move(&engine, &UciEngine::bestMove);
    QVERIFY(startFake(&engine, "normal"));
    QTRY_COMPARE_WITH_TIMEOUT(ready.count(), 1, 2000);

    const int id = engine.play(QString::fromLatin1(kFen), 10);
    QVERIFY(id > 0);
    QTRY_COMPARE_WITH_TIMEOUT(move.count(), 1, 2000);
    const QList<QVariant> result = move.takeFirst();
    QCOMPARE(result[0].toInt(), id);
    QCOMPARE(result[1].toInt(), 12);
    QCOMPARE(result[2].toInt(), 28);
    QCOMPARE(result[3].toInt(), 0);
}

void UciEngineTest::duplicateReadyIsIgnored() {
    UciEngine engine;
    QSignalSpy ready(&engine, &UciEngine::ready);
    QVERIFY(startFake(&engine, "duplicate-ready"));
    QTRY_COMPARE_WITH_TIMEOUT(ready.count(), 1, 2000);
    QTest::qWait(100);
    QCOMPARE(ready.count(), 1);
}

void UciEngineTest::newestPendingSearchWins() {
    UciEngine engine;
    QSignalSpy ready(&engine, &UciEngine::ready);
    QSignalSpy move(&engine, &UciEngine::bestMove);
    QVERIFY(startFake(&engine, "replace"));
    QTRY_COMPARE_WITH_TIMEOUT(ready.count(), 1, 2000);

    const int first = engine.play(QString::fromLatin1(kFen), 1000);
    const QString nextFen =
        QStringLiteral("rnbqkbnr/pppppppp/8/8/4P3/8/PPPP1PPP/RNBQKBNR b KQkq - 0 1");
    const int second = engine.play(nextFen, 10);
    QVERIFY(first > 0);
    QVERIFY(second > first);
    QTRY_COMPARE_WITH_TIMEOUT(move.count(), 1, 2000);
    const QList<QVariant> result = move.takeFirst();
    QCOMPARE(result[0].toInt(), second);
    QCOMPARE(result[1].toInt(), 51);
    QCOMPARE(result[2].toInt(), 35);
}

void UciEngineTest::malformedMoveFailsOnce() {
    UciEngine engine;
    QSignalSpy ready(&engine, &UciEngine::ready);
    QSignalSpy failed(&engine, &UciEngine::failed);
    QVERIFY(startFake(&engine, "invalid"));
    QTRY_COMPARE_WITH_TIMEOUT(ready.count(), 1, 2000);
    QVERIFY(engine.play(QString::fromLatin1(kFen), 10) > 0);
    QTRY_COMPARE_WITH_TIMEOUT(failed.count(), 1, 2000);
    QTest::qWait(100);
    QCOMPARE(failed.count(), 1);
    QVERIFY(!engine.isReady());
}

void UciEngineTest::oversizedOutputFailsOnce() {
    UciEngine engine;
    QSignalSpy ready(&engine, &UciEngine::ready);
    QSignalSpy failed(&engine, &UciEngine::failed);
    QVERIFY(startFake(&engine, "oversized"));
    QTRY_COMPARE_WITH_TIMEOUT(failed.count(), 1, 2000);
    QCOMPARE(ready.count(), 0);
    QCOMPARE(failed.count(), 1);
}

void UciEngineTest::invalidRequestsAreRejected() {
    UciEngine engine;
    QSignalSpy ready(&engine, &UciEngine::ready);
    QVERIFY(startFake(&engine, "normal"));
    QTRY_COMPARE_WITH_TIMEOUT(ready.count(), 1, 2000);
    QCOMPARE(engine.play(QString(), 10), 0);
    QCOMPARE(engine.play(QString::fromLatin1(kFen) + '\n', 10), 0);
    QCOMPARE(engine.play(QString::fromLatin1(kFen), 0), 0);
    QCOMPARE(engine.play(QString::fromLatin1(kFen), 10, {"e2e4\nquit"}), 0);
    QCOMPARE(engine.play(QString::fromLatin1(kFen), 10, {"e7e8x"}), 0);
    QCOMPARE(engine.play(QString::fromLatin1(kFen), 10, {"z9z9"}), 0);
}

void UciEngineTest::processExitFailsOnce() {
    UciEngine engine;
    QSignalSpy ready(&engine, &UciEngine::ready);
    QSignalSpy failed(&engine, &UciEngine::failed);
    QVERIFY(startFake(&engine, "exit"));
    QTRY_COMPARE_WITH_TIMEOUT(ready.count(), 1, 2000);
    QVERIFY(engine.play(QString::fromLatin1(kFen), 10) > 0);
    QTRY_COMPARE_WITH_TIMEOUT(failed.count(), 1, 2000);
    QTest::qWait(100);
    QCOMPARE(failed.count(), 1);
}

void UciEngineTest::shutdownIsBounded() {
    UciEngine engine;
    QSignalSpy ready(&engine, &UciEngine::ready);
    QVERIFY(startFake(&engine, "stubborn"));
    QTRY_COMPARE_WITH_TIMEOUT(ready.count(), 1, 2000);
    QElapsedTimer elapsed;
    elapsed.start();
    engine.shutdown();
    QVERIFY2(elapsed.elapsed() < 2000, qPrintable(QString::number(elapsed.elapsed())));
    QVERIFY(!engine.isRunning());
}

void UciEngineTest::synchronousReadyDeletionIsSafe() {
    QPointer<UciEngine> engine = new UciEngine;
    connect(engine, &UciEngine::ready, engine, [engine] { delete engine.data(); });
    QVERIFY(startFake(engine, "normal"));
    QTRY_VERIFY_WITH_TIMEOUT(engine.isNull(), 2000);
}

void UciEngineTest::historySurvivesUndoRedo() {
    GameController game;
    game.newGame();
    QSignalSpy over(&game, &GameController::gameOver);
    for (int cycle = 0; cycle < 2; ++cycle) {
        game.requestHumanMove(6, 21, 0);
        game.requestHumanMove(62, 45, 0);
        game.requestHumanMove(21, 6, 0);
        game.requestHumanMove(45, 62, 0);
    }
    QCOMPARE(game.board()->status(), 4);
    QCOMPARE(over.count(), 1);
    const QString finalFen = game.board()->fen();
    game.undo();
    QCOMPARE(game.board()->status(), 0);
    game.redo();
    QCOMPARE(game.board()->fen(), finalFen);
    QCOMPARE(game.board()->status(), 4);
    QCOMPARE(over.count(), 2);
    game.undo();
    game.requestHumanMove(45, 39, 0);
    const QString branchFen = game.board()->fen();
    QVERIFY(branchFen != finalFen);
    QCOMPARE(game.board()->status(), 0);
    game.redo();
    QCOMPARE(game.board()->fen(), branchFen);
}

void UciEngineTest::engineReceivesGameHistory() {
    UciEngine engine;
    QSignalSpy ready(&engine, &UciEngine::ready);
    QSignalSpy failed(&engine, &UciEngine::failed);
    const QByteArray expected = QByteArray("position fen ") + kFen +
                                " moves g1f3 g8f6 f3g1 f6g8";
    qputenv("BITFOX_UCI_TEST_POSITION", expected);
    const bool started = startFake(&engine, "history");
    qunsetenv("BITFOX_UCI_TEST_POSITION");
    QVERIFY(started);
    QTRY_COMPARE_WITH_TIMEOUT(ready.count(), 1, 2000);
    GameController game;
    game.newGame();
    game.requestHumanMove(6, 21, 0);
    game.requestHumanMove(62, 45, 0);
    game.requestHumanMove(21, 6, 0);
    game.requestHumanMove(45, 62, 0);
    game.requestHumanMove(12, 20, 0);
    game.undo();
    game.setEngine(0, &engine);
    game.nudge();
    QTRY_COMPARE_WITH_TIMEOUT(game.board()->pieceAt(28), 0, 2000);
    QCOMPARE(failed.count(), 0);
    game.setEngine(0, nullptr);
}

void UciEngineTest::newGameResetsTheEngine() {
    UciEngine engine;
    QSignalSpy ready(&engine, &UciEngine::ready);
    QSignalSpy failed(&engine, &UciEngine::failed);
    QVERIFY(startFake(&engine, "newgame"));
    QTRY_COMPARE_WITH_TIMEOUT(ready.count(), 1, 2000);
    GameController game;
    game.setEngine(0, &engine);
    game.newGame();
    QTRY_COMPARE_WITH_TIMEOUT(game.board()->pieceAt(28), 0, 2000);
    game.requestHumanMove(52, 36, 0);
    QTRY_COMPARE_WITH_TIMEOUT(game.board()->pieceAt(21), 1, 2000);
    QCOMPARE(failed.count(), 0);
    game.setEngine(0, nullptr);
}

void UciEngineTest::newGameDuringSearchDiscardsTheStoppedMove() {
    UciEngine engine;
    QSignalSpy ready(&engine, &UciEngine::ready);
    QSignalSpy failed(&engine, &UciEngine::failed);
    QVERIFY(startFake(&engine, "newgame-active"));
    QTRY_COMPARE_WITH_TIMEOUT(ready.count(), 1, 2000);
    GameController game;
    QSignalSpy moves(&game, &GameController::movePlayed);
    game.setEngine(0, &engine);
    game.newGame();
    QTRY_COMPARE_WITH_TIMEOUT(engine.name(), QStringLiteral("active-newgame-search"), 2000);
    QCOMPARE(moves.count(), 0);
    game.newGame();
    QTRY_COMPARE_WITH_TIMEOUT(moves.count(), 1, 2000);
    QCOMPARE(game.board()->pieceAt(27), 0);
    QCOMPARE(game.board()->pieceAt(12), 0);
    QCOMPARE(game.board()->pieceAt(28), -1);
    QCOMPARE(game.board()->sideToMove(), 1);
    const QList<QVariant> move = moves.takeFirst();
    QCOMPARE(move[0].toInt(), 11);
    QCOMPARE(move[1].toInt(), 27);
    QCOMPARE(failed.count(), 0);
    game.setEngine(0, nullptr);
}

void UciEngineTest::undoingTheEnginesFirstMoveResumesTheEngine() {
    UciEngine engine;
    QSignalSpy ready(&engine, &UciEngine::ready);
    QSignalSpy failed(&engine, &UciEngine::failed);
    QVERIFY(startFake(&engine, "opening"));
    QTRY_COMPARE_WITH_TIMEOUT(ready.count(), 1, 2000);
    GameController game;
    game.setEngine(0, &engine);
    game.newGame();
    QTRY_COMPARE_WITH_TIMEOUT(game.board()->pieceAt(28), 0, 2000);
    game.undo();
    QCOMPARE(game.board()->pieceAt(12), 0);
    QTRY_COMPARE_WITH_TIMEOUT(game.board()->pieceAt(28), 0, 2000);
    QCOMPARE(game.board()->sideToMove(), 1);
    QCOMPARE(failed.count(), 0);
    game.setEngine(0, nullptr);
}

void UciEngineTest::queuedHistoryIsPreserved() {
    UciEngine engine;
    QSignalSpy ready(&engine, &UciEngine::ready);
    QSignalSpy move(&engine, &UciEngine::bestMove);
    QSignalSpy failed(&engine, &UciEngine::failed);
    qputenv("BITFOX_UCI_TEST_POSITION", QByteArray("position fen ") + kFen + " moves e2e4");
    const bool started = startFake(&engine, "replace-history");
    qunsetenv("BITFOX_UCI_TEST_POSITION");
    QVERIFY(started);
    QTRY_COMPARE_WITH_TIMEOUT(ready.count(), 1, 2000);
    QVERIFY(engine.play(QString::fromLatin1(kFen), 1000) > 0);
    QVERIFY(engine.play(QString::fromLatin1(kFen), 10, {"d2d4"}) > 0);
    const int last = engine.play(QString::fromLatin1(kFen), 10, {"e2e4"});
    QVERIFY(last > 0);
    QTRY_COMPARE_WITH_TIMEOUT(move.count(), 1, 2000);
    QCOMPARE(move.takeFirst()[0].toInt(), last);
    QCOMPARE(failed.count(), 0);
}

void UciEngineTest::illegalMoveReleasesTheSeat() {
    UciEngine engine;
    QSignalSpy ready(&engine, &UciEngine::ready);
    QSignalSpy failed(&engine, &UciEngine::failed);
    QVERIFY(startFake(&engine, "illegal"));
    QTRY_COMPARE_WITH_TIMEOUT(ready.count(), 1, 2000);
    GameController game;
    game.newGame();
    connect(&engine, &UciEngine::failed, &game, [&game] {
        game.setEngine(0, nullptr);
        game.nudge();
    });
    game.setEngine(0, &engine);
    game.nudge();
    QTRY_COMPARE_WITH_TIMEOUT(failed.count(), 1, 2000);
    QVERIFY(!engine.isReady());
    QVERIFY(game.isHuman(0));
    QCOMPARE(game.board()->fen(), QString::fromLatin1(kFen));
    game.requestHumanMove(12, 28, 0);
    QCOMPARE(game.board()->pieceAt(28), 0);
}

void UciEngineTest::promotionSuffixIsCaseInsensitive() {
    UciEngine engine;
    QSignalSpy ready(&engine, &UciEngine::ready);
    QSignalSpy move(&engine, &UciEngine::bestMove);
    QSignalSpy failed(&engine, &UciEngine::failed);
    QVERIFY(startFake(&engine, "promotion"));
    QTRY_COMPARE_WITH_TIMEOUT(ready.count(), 1, 2000);
    const int id = engine.play(QStringLiteral("7k/P7/8/8/8/8/8/7K w - - 0 1"), 10);
    QVERIFY(id > 0);
    QTRY_COMPARE_WITH_TIMEOUT(move.count(), 1, 2000);
    const QList<QVariant> result = move.takeFirst();
    QCOMPARE(result[0].toInt(), id);
    QCOMPARE(result[1].toInt(), 48);
    QCOMPARE(result[2].toInt(), 56);
    QCOMPARE(result[3].toInt(), 4);
    QCOMPARE(failed.count(), 0);
}

void UciEngineTest::pgnFollowsHistoryCursor() {
    GameController game;
    game.newGame();
    game.requestHumanMove(12, 28, 0);
    game.requestHumanMove(52, 36, 0);
    game.requestHumanMove(6, 21, 0);
    game.undo();
    QString pgn = game.pgn("White", "Black", "*");
    QVERIFY(pgn.contains("[Round \"?\"]"));
    QVERIFY(pgn.endsWith("1. e4 e5 *\n"));
    game.redo();
    QVERIFY(game.pgn("White", "Black", "*").endsWith("1. e4 e5 2. Nf3 *\n"));
    game.undo();
    game.requestHumanMove(1, 18, 0);
    QVERIFY(game.pgn("White", "Black", "*").endsWith("1. e4 e5 2. Nc3 *\n"));
    game.newGame();
    QVERIFY(game.pgn("White", "Black", "*").endsWith("\n\n*\n"));
}

void UciEngineTest::capturesSurvivePromotionUndoAndBranch() {
    GameController game;
    game.newGame();
    const QStringList moves = {"a2a4", "h7h5", "a4a5", "h5h4", "a5a6", "h4h3",
                               "a6b7", "h3g2", "b7a8q", "c8b7", "h2h3", "b7a8"};
    for (const QString &move : moves) {
        const int from = CoreBoard::squareFromName(move.left(2));
        const int to = CoreBoard::squareFromName(move.mid(2, 2));
        QVERIFY(game.board()->legalTo(from).contains(to));
        game.requestHumanMove(from, to, move.size() == 5 ? 4 : 0);
    }
    QCOMPARE(game.capturedBy(0), QVector<int>({6, 9}));
    QCOMPARE(game.capturedBy(1), QVector<int>({0, 4}));
    game.undo();
    QCOMPARE(game.capturedBy(1), QVector<int>({0}));
    game.redo();
    QCOMPARE(game.capturedBy(1), QVector<int>({0, 4}));
    game.undo();
    game.requestHumanMove(62, 45, 0);
    game.redo();
    QCOMPARE(game.capturedBy(1), QVector<int>({0}));
    game.newGame();
    QVERIFY(game.capturedBy(0).isEmpty());
    QVERIFY(game.capturedBy(1).isEmpty());
}

void UciEngineTest::enPassantProjectionRemovesCapturedPawn() {
    GameController game;
    game.newGame();
    game.requestHumanMove(12, 28, 0);
    game.requestHumanMove(48, 40, 0);
    game.requestHumanMove(28, 36, 0);
    game.setPremove(36, 43, 0);
    bool projected = false;
    connect(&game, &GameController::premovesChanged, &game,
            [&game, &projected](const QVector<QPair<int, int>> &moves) {
                if (!moves.isEmpty() && game.board()->sideToMove() == 0) {
                    projected = game.premoveBoard()->pieceAt(35) == -1 &&
                                game.premoveBoard()->pieceAt(43) == 0;
                }
            });
    game.requestHumanMove(51, 35, 0);
    QVERIFY(projected);
    QCOMPARE(game.board()->pieceAt(35), -1);
    QCOMPARE(game.board()->pieceAt(43), 0);
    QCOMPARE(game.capturedBy(0), QVector<int>({6}));
}

void UciEngineTest::premoveProjectionCanAwaitAnUnpin() {
    GameController game;
    game.newGame();
    const QStringList moves = {"d2d4", "e7e6", "b1c3", "f8b4", "a2a3"};
    for (const QString &move : moves) {
        game.requestHumanMove(CoreBoard::squareFromName(move.left(2)),
                              CoreBoard::squareFromName(move.mid(2, 2)), 0);
    }
    QCOMPARE(game.board()->sideToMove(), 1);
    game.setPremove(18, 28, 0);
    QCOMPARE(game.premoveBoard()->pieceAt(18), -1);
    QCOMPARE(game.premoveBoard()->pieceAt(28), 1);
    game.requestHumanMove(25, 34, 0);
    QCOMPARE(game.board()->pieceAt(28), 1);
}

void UciEngineTest::cancelledPromotionPreservesNextAnimation() {
    CoreBoard board;
    QVERIFY(board.setFen(QStringLiteral("7k/P7/8/8/8/8/8/7K w - - 0 1")));
    BoardView view;
    view.setBoard(&board);
    view.resize(640, 640);
    view.show();
    QSignalSpy moves(&view, &BoardView::moveRequested);
    QTest::mousePress(&view, Qt::LeftButton, Qt::NoModifier, QPoint(40, 120));
    QTest::mouseMove(&view, QPoint(40, 40));
    QTimer::singleShot(0, [] {
        if (QWidget *popup = QApplication::activePopupWidget()) {
            popup->close();
        }
    });
    QTest::mouseRelease(&view, Qt::LeftButton, Qt::NoModifier, QPoint(40, 40));
    QCOMPARE(moves.count(), 0);
    CcMoveInfo info{};
    QVERIFY(board.apply(48, 56, 4, &info));
    view.animateMove(48, 56);
    QTimer *timer = view.findChild<QTimer *>();
    QVERIFY(timer != nullptr);
    QVERIFY(timer->isActive());
}

void UciEngineTest::synchronousWriteFailureReleasesSeat() {
    UciEngine engine;
    QSignalSpy ready(&engine, &UciEngine::ready);
    QSignalSpy failed(&engine, &UciEngine::failed);
    QVERIFY(startFake(&engine, "normal"));
    QTRY_COMPARE_WITH_TIMEOUT(ready.count(), 1, 2000);
    GameController game;
    game.newGame();
    connect(&engine, &UciEngine::failed, &game, [&game] {
        game.setEngine(0, nullptr);
        game.nudge();
    });
    game.setEngine(0, &engine);
    QProcess *process = engine.findChild<QProcess *>();
    QVERIFY(process);
    process->closeWriteChannel();
    game.nudge();
    QTRY_COMPARE_WITH_TIMEOUT(failed.count(), 1, 2000);
    QVERIFY(game.isHuman(0));
    game.requestHumanMove(12, 28, 0);
    QCOMPARE(game.board()->pieceAt(28), 0);
}

int main(int argc, char **argv) {
    const QByteArray mode = qgetenv("BITFOX_UCI_TEST_MODE");
    if (!mode.isEmpty()) {
        return runFakeEngine(mode.toStdString());
    }
    QApplication app(argc, argv);
    UciEngineTest test;
    return QTest::qExec(&test, argc, argv);
}

#include "UciEngineTest.moc"
