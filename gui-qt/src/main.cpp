#include <QApplication>
#include <QFile>
#include <QLocale>
#include <QPalette>

#include "ui/MainWindow.h"

int main(int argc, char *argv[]) {
    QApplication app(argc, argv);
    QLocale::setDefault(QLocale(QLocale::English, QLocale::UnitedStates));
    app.setApplicationName("Bitfox");
    app.setStyle("Fusion");

    QPalette pal;
    pal.setColor(QPalette::Window, QColor(0x31, 0x2e, 0x2b));
    pal.setColor(QPalette::Base, QColor(0x21, 0x1f, 0x1d));
    pal.setColor(QPalette::Text, QColor(0xe9, 0xe9, 0xe4));
    pal.setColor(QPalette::WindowText, QColor(0xe9, 0xe9, 0xe4));
    pal.setColor(QPalette::Button, QColor(0x3a, 0x38, 0x35));
    pal.setColor(QPalette::ButtonText, QColor(0xe9, 0xe9, 0xe4));
    pal.setColor(QPalette::Highlight, QColor(0x81, 0xb6, 0x4c));
    pal.setColor(QPalette::HighlightedText, QColor(0xff, 0xff, 0xff));
    app.setPalette(pal);

    QFile style(":/style.qss");
    if (style.open(QIODevice::ReadOnly)) {
        app.setStyleSheet(QString::fromUtf8(style.readAll()));
    }

    MainWindow window;
    window.show();
    return app.exec();
}
