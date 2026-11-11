#include <QApplication>
#include <QDir>
#include <QElapsedTimer>
#include <QFile>
#include <QScopeGuard>
#include <QSettings>
#include <QSignalSpy>
#include <QStandardPaths>
#include <QTemporaryDir>
#include <QThread>
#include <QTimer>
#include <QUuid>
#include <QtTest>

#include <iostream>
#include <string>

#include "board/BoardView.h"
#include "core/CoreBoard.h"
#include "engine/UciEngine.h"
#include "engine/EngineCatalog.h"
#include "engine/ReleaseManager.h"
#include "game/GameController.h"
#include "ui/MainWindow.h"

namespace {
int fakeEngine() {
    int searches = 0;
    std::string command;
    while (std::getline(std::cin, command)) {
        if (command == "uci") {
            std::cout << "id name Bitfox window test engine\nuciok\n" << std::flush;
        } else if (command == "isready") {
            std::cout << "readyok\n" << std::flush;
        } else if (command.rfind("go movetime ", 0) == 0) {
            ++searches;
            std::cout << "id name window-search-" << searches << '\n' << std::flush;
            if (searches == 2) {
                std::cout << "bestmove d2d4\n" << std::flush;
            }
        } else if (command == "stop") {
            std::cout << "bestmove e2e4\n" << std::flush;
        } else if (command == "quit") {
            return 0;
        }
    }
    return 0;
}
}

class MainWindowTest : public QObject {
    Q_OBJECT

private slots:
    void initTestCase();
    void cleanup();
    void hintKeepsUiResponsive();
    void changedPositionDiscardsHint();
    void newGameDiscardsHint();
    void repeatedHintsAreCoalesced();
    void destructionDuringHintRestoresCursor();
    void newGameDuringEngineSearchDiscardsOldMove();
    void engineVersionsFollowSemanticOrder();
#if defined(Q_OS_WIN)
    void versionPackagesAreDiscoveredWithoutStagingFiles();
#endif

private:
    QTemporaryDir m_settings;
};

void MainWindowTest::initTestCase() {
    QVERIFY(m_settings.isValid());
    QCoreApplication::setOrganizationName(QStringLiteral("BitfoxTests"));
    QCoreApplication::setApplicationName(QStringLiteral("MainWindowTest"));
    QStandardPaths::setTestModeEnabled(true);
    QSettings::setDefaultFormat(QSettings::IniFormat);
    QSettings::setPath(QSettings::IniFormat, QSettings::UserScope, m_settings.path());
    QSettings settings;
    settings.setValue(QStringLiteral("autoUpdate"), false);
    settings.setValue(QStringLiteral("appUpdates/automatic"), false);
    settings.setValue(QStringLiteral("soundOn"), false);
    settings.sync();
    QCOMPARE(settings.status(), QSettings::NoError);
}

void MainWindowTest::cleanup() {
    while (QApplication::overrideCursor()) {
        QApplication::restoreOverrideCursor();
    }
}

void MainWindowTest::hintKeepsUiResponsive() {
    MainWindow window;
    int pulses = 0;
    QTimer pulse;
    connect(&pulse, &QTimer::timeout, this, [&pulses] { ++pulses; });
    pulse.start(1);
    window.showHint();
    QVERIFY(window.m_hintThread);
    QTRY_VERIFY_WITH_TIMEOUT(pulses > 0, 300);
    QVERIFY(window.m_hintThread);
    QTRY_VERIFY_WITH_TIMEOUT(window.m_hintThread == nullptr, 3000);
    QVERIFY(window.m_boardView->m_hintFrom >= 0);
    QVERIFY(window.m_game->board()->legalTo(window.m_boardView->m_hintFrom)
                .contains(window.m_boardView->m_hintTo));
    QVERIFY(QApplication::overrideCursor() == nullptr);
}

void MainWindowTest::changedPositionDiscardsHint() {
    MainWindow window;
    window.showHint();
    window.m_game->requestHumanMove(12, 28, 0);
    QCOMPARE(window.m_game->board()->sideToMove(), 1);
    QTRY_VERIFY_WITH_TIMEOUT(window.m_hintThread == nullptr, 3000);
    QCOMPARE(window.m_boardView->m_hintFrom, -1);
    QCOMPARE(window.m_boardView->m_hintTo, -1);
}

void MainWindowTest::newGameDiscardsHint() {
    MainWindow window;
    window.m_game->requestHumanMove(12, 28, 0);
    window.showHint();
    window.onNewGame();
    QCOMPARE(window.m_game->board()->pieceAt(12), 0);
    QCOMPARE(window.m_game->board()->pieceAt(28), -1);
    QTRY_VERIFY_WITH_TIMEOUT(window.m_hintThread == nullptr, 3000);
    QCOMPARE(window.m_boardView->m_hintFrom, -1);
    QCOMPARE(window.m_boardView->m_hintTo, -1);
}

void MainWindowTest::repeatedHintsAreCoalesced() {
    MainWindow window;
    window.showHint();
    QThread *first = window.m_hintThread;
    QVERIFY(first);
    QSignalSpy finished(first, &QThread::finished);
    window.showHint();
    window.showHint();
    QCOMPARE(window.m_hintThread, first);
    QVERIFY(window.m_hintRequested);
    QTRY_COMPARE_WITH_TIMEOUT(finished.count(), 1, 3000);
    QTRY_VERIFY_WITH_TIMEOUT(window.m_hintThread == nullptr, 3000);
    QVERIFY(!window.m_hintRequested);
    QVERIFY(QApplication::overrideCursor() == nullptr);
    QVERIFY(window.m_boardView->m_hintFrom >= 0);
}

void MainWindowTest::destructionDuringHintRestoresCursor() {
    auto *window = new MainWindow;
    window->showHint();
    QVERIFY(window->m_hintThread);
    QVERIFY(QApplication::overrideCursor());
    QElapsedTimer elapsed;
    elapsed.start();
    delete window;
    QVERIFY2(elapsed.elapsed() < 3000, qPrintable(QString::number(elapsed.elapsed())));
    QVERIFY(QApplication::overrideCursor() == nullptr);
    QCoreApplication::processEvents();
    QApplication::setOverrideCursor(Qt::CrossCursor);
    window = new MainWindow;
    window->showHint();
    QCOMPARE(QApplication::overrideCursor()->shape(), Qt::WaitCursor);
    delete window;
    QVERIFY(QApplication::overrideCursor());
    QCOMPARE(QApplication::overrideCursor()->shape(), Qt::CrossCursor);
    QCoreApplication::processEvents();
    QCOMPARE(QApplication::overrideCursor()->shape(), Qt::CrossCursor);
    QApplication::restoreOverrideCursor();
}

void MainWindowTest::newGameDuringEngineSearchDiscardsOldMove() {
    MainWindow window;
    QSignalSpy moves(window.m_game, &GameController::movePlayed);
    qputenv("BITFOX_WINDOW_TEST_ENGINE", "1");
    window.onEngineSelected(0, QCoreApplication::applicationFilePath());
    qunsetenv("BITFOX_WINDOW_TEST_ENGINE");
    QVERIFY(window.m_engines[0]);
    QTRY_COMPARE_WITH_TIMEOUT(window.m_engines[0]->name(),
                             QStringLiteral("window-search-1"), 2000);
    QCOMPARE(moves.count(), 0);
    window.onNewGame();
    QTRY_COMPARE_WITH_TIMEOUT(moves.count(), 1, 2000);
    QCOMPARE(window.m_game->board()->pieceAt(12), 0);
    QCOMPARE(window.m_game->board()->pieceAt(28), -1);
    QCOMPARE(window.m_game->board()->pieceAt(27), 0);
    const QList<QVariant> move = moves.takeFirst();
    QCOMPARE(move[0].toInt(), 11);
    QCOMPARE(move[1].toInt(), 27);
    window.onEngineSelected(0, QString());
}

void MainWindowTest::engineVersionsFollowSemanticOrder() {
    QTemporaryDir engines;
    QVERIFY(engines.isValid());
    const QString directory = engines.path();
    const QString prefix = "Catalog-" + QUuid::createUuid().toString(QUuid::WithoutBraces) + ' ';
    QString suffix;
#if defined(Q_OS_WIN)
    suffix = ".exe";
#endif
    const QStringList expected = {"Custom 3", "Custom 2.6.0.1", "Bitfox 2.6.0", "Custom 2.6",
                                  "Bitfox 2.6.0-rc.10", "Bitfox 2.6.0-rc.9",
                                  "Custom 2.5.0.2", "Bitfox 2.5.0+build.8"};
    QStringList owned;
    const auto cleanup = qScopeGuard([&owned] {
        for (const QString &path : owned) QFile::remove(path);
    });
    for (const QString &name : expected) {
        const QString path = QDir(directory).filePath(prefix + name + suffix);
        QFile file(path);
        QVERIFY2(file.open(QIODevice::WriteOnly | QIODevice::NewOnly), qPrintable(file.errorString()));
        owned.append(path);
        QCOMPARE(file.write("fixture"), qint64(7));
        file.close();
#if !defined(Q_OS_WIN)
        QVERIFY(file.setPermissions(QFile::ReadOwner | QFile::WriteOwner | QFile::ExeOwner));
#endif
    }
    QStringList actual;
    for (const EngineEntry &entry : EngineCatalog::discover({directory})) {
        if (entry.name.startsWith(prefix))
            actual.append(entry.name.mid(prefix.size()).chopped(suffix.size()));
    }
    QCOMPARE(actual, expected);
}

#if defined(Q_OS_WIN)
void MainWindowTest::versionPackagesAreDiscoveredWithoutStagingFiles() {
    QTemporaryDir engines;
    QVERIFY(engines.isValid());
    const QDir root(engines.path());
    QVERIFY(root.mkpath("Bitfox 2.6.0"));
    QVERIFY(root.mkpath("Bitfox 2.5.1-rc.exe"));
    QVERIFY(root.mkpath("install-incomplete"));
    const QString packaged = root.filePath("Bitfox 2.6.0/Bitfox 2.6.0.exe");
    for (const QString &path : {packaged, root.filePath("Bitfox 2.6.0.exe"),
                                root.filePath("Bitfox 2.5.0.exe"),
                                root.filePath("Bitfox 2.5.1-rc.exe/Bitfox 2.5.1-rc.exe.exe"),
                                root.filePath("Bitfox 2.5.1-rc.exe/RUNTIME_DEPENDENCIES.json"),
                                root.filePath("install-incomplete/Bitfox 9.0.0.exe"),
                                root.filePath("Bitfox 2.6.0/RUNTIME_DEPENDENCIES.json"),
                                root.filePath("Bitfox 2.6.0/vcruntime140.dll")}) {
        QFile file(path);
        QVERIFY(file.open(QIODevice::WriteOnly | QIODevice::NewOnly));
        QCOMPARE(file.write("fixture"), qint64(7));
    }
    const auto entries = EngineCatalog::discover({root.path()});
    QCOMPARE(entries.size(), 3);
    QCOMPARE(entries[0].name, QString("Bitfox 2.6.0.exe"));
    QCOMPARE(entries[0].path, packaged);
    QCOMPARE(entries[1].name, QString("Bitfox 2.5.1-rc.exe.exe"));
    QCOMPARE(entries[2].name, QString("Bitfox 2.5.0.exe"));
}
#endif

int main(int argc, char **argv) {
    if (!qgetenv("BITFOX_WINDOW_TEST_ENGINE").isEmpty()) {
        return fakeEngine();
    }
    QApplication app(argc, argv);
    MainWindowTest test;
    return QTest::qExec(&test, argc, argv);
}

#include "MainWindowTest.moc"
