#pragma once

#include <QString>
#include <QStringList>
#include <QVector>

struct EngineEntry {
    QString name;
    QString path;
};

class EngineCatalog {
public:
    static QVector<EngineEntry> discover(const QStringList &directories = {});
    static QVector<EngineEntry> scanDir(const QString &dir);

private:
    static bool looksLikeEngine(const QString &path);
};
