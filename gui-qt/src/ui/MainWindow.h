#pragma once

#include <QMainWindow>
#include <QStringList>

class BoardView;
class SidePanel;
class GameController;
class UciEngine;
class SoundBank;
class AppUpdater;
class AppUpdateDialog;
class QThread;

class MainWindow : public QMainWindow {
    Q_OBJECT
public:
    explicit MainWindow(QWidget *parent = nullptr);
    ~MainWindow() override;

protected:
    void keyPressEvent(QKeyEvent *event) override;

private slots:
    void onEngineSelected(int color, const QString &path);
    void onNewGame();
    void onHistoryRebuilt(const QStringList &moves, int lastFrom, int lastTo);
    void onGameOver(int status, const QString &text);
    void updateCaptured();
    void showHint();
    void openSettings();
    void openVersions();
    void openAppUpdates();

private:
    friend class MainWindowTest;

    void updateHumanColors();
    void loadSettings();
    void checkForUpdates();

    BoardView *m_boardView;
    SidePanel *m_panel;
    GameController *m_game;
    SoundBank *m_sound;
    AppUpdater *m_appUpdater;
    AppUpdateDialog *m_updateDialog = nullptr;
    QThread *m_hintThread = nullptr;
    bool m_hintRequested = false;
    UciEngine *m_engines[2] = {nullptr, nullptr};
    bool m_soundOn = true;
    bool m_premoveOn = true;
    bool m_autoUpdate = false;
};
