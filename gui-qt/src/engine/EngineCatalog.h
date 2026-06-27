#pragma once

#include <QString>
#include <QVector>

struct EngineEntry {
    QString name;
    QString path;
};

class EngineCatalog {
public:
    static QVector<EngineEntry> discover();
    static QVector<EngineEntry> scanDir(const QString &dir);

private:
    static bool looksLikeEngine(const QString &path);
};
