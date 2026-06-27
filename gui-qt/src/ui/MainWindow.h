#pragma once

#include <QMainWindow>
#include <QStringList>

class BoardView;
class SidePanel;
class GameController;
class UciEngine;
class SoundBank;

class MainWindow : public QMainWindow {
    Q_OBJECT
public:
    explicit MainWindow(QWidget *parent = nullptr);

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

private:
    void updateHumanColors();

    BoardView *m_boardView;
    SidePanel *m_panel;
    GameController *m_game;
    SoundBank *m_sound;
    UciEngine *m_engines[2] = {nullptr, nullptr};
    bool m_soundOn = true;
    bool m_premoveOn = true;
};
