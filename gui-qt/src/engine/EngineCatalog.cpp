#include "engine/EngineCatalog.h"

#include <QCoreApplication>
#include <QDir>
#include <QFileInfo>
#include <QRegularExpression>
#include <QSet>

#include <algorithm>

static QVector<int> versionOf(const QString &name) {
    QVector<int> v;
    static const QRegularExpression re(QStringLiteral("(\\d+(?:\\.\\d+)*)\\s*$"));
    const QRegularExpressionMatch m = re.match(name);
    if (m.hasMatch()) {
        const QStringList parts = m.captured(1).split('.');
        for (const QString &part : parts) {
            v.append(part.toInt());
        }
    }
    return v;
}

static bool newerThan(const EngineEntry &a, const EngineEntry &b) {
    const QVector<int> va = versionOf(a.name);
    const QVector<int> vb = versionOf(b.name);
    const int n = qMax(va.size(), vb.size());
    for (int i = 0; i < n; ++i) {
        const int x = i < va.size() ? va[i] : 0;
        const int y = i < vb.size() ? vb[i] : 0;
        if (x != y) {
            return x > y;
        }
    }
    return a.name < b.name;
}

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
    std::sort(all.begin(), all.end(), newerThan);
    return all;
}
