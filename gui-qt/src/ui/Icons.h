#pragma once

#include <QColor>
#include <QIcon>
#include <QString>

namespace Icons {
QIcon make(const QString &name, const QColor &color = QColor(0xe9, 0xe9, 0xe4),
           int size = 28);
}
