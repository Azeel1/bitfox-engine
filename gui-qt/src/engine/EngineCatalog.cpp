#include "engine/EngineCatalog.h"

#include <QCoreApplication>
#include <QDir>
#include <QFileInfo>
#include <QSet>

bool EngineCatalog::looksLikeEngine(const QString &path) {
    QFileInfo info(path);
    if (!info.isFile() || !info.isExecutable()) {
        return false;
    }
    static const QSet<QString> skip = {"dylib", "so",  "dll", "rlib",
                                       "d",     "rmeta", "a",   "txt"};
    return !skip.contains(info.suffix().toLower());
}

QVector<EngineEntry> EngineCatalog::scanDir(const QString &dir) {
    QVector<EngineEntry> found;
    QDir d(dir);
    if (!d.exists()) {
        return found;
    }
    const QFileInfoList entries = d.entryInfoList(QDir::Files | QDir::NoDotAndDotDot);
    for (const QFileInfo &info : entries) {
        if (looksLikeEngine(info.absoluteFilePath())) {
            found.append({info.fileName(), info.absoluteFilePath()});
        }
    }
    return found;
}

QVector<EngineEntry> EngineCatalog::discover() {
    QVector<EngineEntry> all;
    QSet<QString> seen;

    QStringList roots;
    const QString appDir = QCoreApplication::applicationDirPath();
    roots << appDir + "/engines";
    roots << appDir + "/../engines";
    roots << QDir::currentPath() + "/engines";

    for (const QString &root : roots) {
        for (const EngineEntry &e : scanDir(root)) {
            if (!seen.contains(e.path)) {
                seen.insert(e.path);
                all.append(e);
            }
        }
    }
    return all;
}
