// qml_startup.cpp — Qt/QML startup diagnostics for the native client.
//
// Mirrors the app_icon.cpp pattern: plain C++ compiled via `cc` in build.rs
// and called from main.rs through extern "C" shims.

#include <QClipboard>
#include <QDateTime>
#include <QDir>
#include <QFileInfo>
#include <QGuiApplication>
#include <QImage>
#include <QMimeData>
#include <QQmlApplicationEngine>
#include <QQuickWindow>
#include <QUrl>
#include <QWindow>
#include <cstdio>
#include <cstring>

#if defined(Q_OS_WIN)
// Defined in window_chrome.cpp (linked only on Windows qt-ui builds).
extern "C" void doubleslash_enable_windows_snap(void *qwindow_ptr);
#endif

static void qtLogToStderr(QtMsgType type, const QMessageLogContext &ctx, const QString &msg) {
    const char *level = "INFO";
    switch (type) {
    case QtDebugMsg:    level = "DEBUG"; break;
    case QtInfoMsg:     level = "INFO";  break;
    case QtWarningMsg:  level = "WARN";  break;
    case QtCriticalMsg: level = "CRIT";  break;
    case QtFatalMsg:    level = "FATAL"; break;
    }
    if (ctx.category && ctx.file && ctx.line > 0) {
        fprintf(stderr, "[QT %s] %s (%s:%d)\n",
                level, msg.toUtf8().constData(), ctx.file, ctx.line);
    } else {
        fprintf(stderr, "[QT %s] %s\n", level, msg.toUtf8().constData());
    }
    if (type == QtFatalMsg) {
        abort();
    }
}

extern "C" void doubleslash_install_qt_message_handler(void) {
    qInstallMessageHandler(qtLogToStderr);
}

/// Product identity for the native caption, taskbar, and alt-tab. Must run
/// after QGuiApplication exists and before the first window is shown; an
/// empty ApplicationWindow title otherwise falls back to the executable
/// basename (still `doubleslash-client` from cargo).
extern "C" void doubleslash_set_app_identity(void) {
    QGuiApplication::setApplicationName("DoubleSlash");
    QGuiApplication::setApplicationDisplayName("DoubleSlash");
    QGuiApplication::setOrganizationName("DoubleSlash");
    QGuiApplication::setOrganizationDomain("doubleslash.space");
}

static bool isImageFileSuffix(const QString &suffix) {
    static const char *const kSuffixes[] = {"png", "jpg", "jpeg", "gif", "webp", "bmp"};
    const QString lower = suffix.toLower();
    for (const char *s : kSuffixes) {
        if (lower == QLatin1String(s)) {
            return true;
        }
    }
    return false;
}

/// Resolve the image on the clipboard to a local file and write its `file:`
/// URL (UTF-8, NUL-terminated) into `out`. A copied image file is used where it is;
/// bitmap data is saved as a PNG under `dir_utf8`. Text wins over bitmap data,
/// because Office and browsers put a rendered picture beside copied text.
/// Returns the path length, or 0 when the clipboard holds no image.
extern "C" size_t doubleslash_clipboard_take_image(const char *dir_utf8, char *out, size_t out_len) {
    if (!out || out_len == 0) {
        return 0;
    }
    out[0] = '\0';
    QClipboard *clipboard = QGuiApplication::clipboard();
    const QMimeData *mime = clipboard ? clipboard->mimeData() : nullptr;
    if (!mime) {
        return 0;
    }

    QString path;
    if (mime->hasUrls()) {
        for (const QUrl &url : mime->urls()) {
            if (!url.isLocalFile()) {
                continue;
            }
            const QFileInfo info(url.toLocalFile());
            if (info.isFile() && isImageFileSuffix(info.suffix())) {
                path = info.absoluteFilePath();
                break;
            }
        }
    }
    if (path.isEmpty() && mime->hasImage() && (!mime->hasText() || mime->text().trimmed().isEmpty())) {
        const QImage image = qvariant_cast<QImage>(mime->imageData());
        if (image.isNull() || !dir_utf8) {
            return 0;
        }
        const QString dir = QString::fromUtf8(dir_utf8);
        if (!QDir().mkpath(dir)) {
            return 0;
        }
        const QString stamp = QDateTime::currentDateTime().toString(QStringLiteral("yyyyMMdd-HHmmss-zzz"));
        const QString candidate = QDir(dir).filePath(QStringLiteral("pasted-%1.png").arg(stamp));
        if (!image.save(candidate, "PNG")) {
            return 0;
        }
        path = candidate;
    }
    if (path.isEmpty()) {
        return 0;
    }
    const QByteArray utf8 = QUrl::fromLocalFile(path).toString(QUrl::FullyEncoded).toUtf8();
    if (static_cast<size_t>(utf8.size()) >= out_len) {
        return 0;
    }
    std::memcpy(out, utf8.constData(), static_cast<size_t>(utf8.size()));
    out[utf8.size()] = '\0';
    return static_cast<size_t>(utf8.size());
}

extern "C" void doubleslash_qml_post_load_check(QQmlApplicationEngine *engine) {
    if (!engine) {
        fprintf(stderr, "[QML] engine pointer is null\n");
        return;
    }

    engine->setOutputWarningsToStandardError(true);

    const auto roots = engine->rootObjects();
    fprintf(stderr, "[QML] root object count: %d\n", static_cast<int>(roots.size()));

    bool sawWindow = false;
    for (QObject *obj : roots) {
        if (!obj) {
            continue;
        }
        fprintf(stderr, "[QML] root object: %s\n", obj->metaObject()->className());

        if (auto *quickWin = qobject_cast<QQuickWindow *>(obj)) {
            sawWindow = true;
            fprintf(stderr,
                    "[QML] QQuickWindow visible=%d size=%dx%d pos=(%d,%d)\n",
                    quickWin->isVisible(),
                    quickWin->width(),
                    quickWin->height(),
                    quickWin->x(),
                    quickWin->y());
#if defined(Q_OS_WIN)
            // Force HWND creation then install snap-friendly frame chrome.
            // Re-arm on show is WM_SHOWWINDOW in window_chrome.cpp.
            (void)quickWin->winId();
            doubleslash_enable_windows_snap(static_cast<QWindow *>(quickWin));
#endif
        } else if (auto *win = qobject_cast<QWindow *>(obj)) {
            sawWindow = true;
            fprintf(stderr,
                    "[QML] QWindow visible=%d size=%dx%d pos=(%d,%d)\n",
                    win->isVisible(),
                    win->width(),
                    win->height(),
                    win->x(),
                    win->y());
#if defined(Q_OS_WIN)
            (void)win->winId();
            doubleslash_enable_windows_snap(win);
#endif
        }
    }

    if (roots.isEmpty()) {
        fprintf(stderr,
                "[QML] ERROR: MainWindow failed to load — no root objects were created\n");
    } else if (!sawWindow) {
        fprintf(stderr,
                "[QML] WARN: root objects exist but none are QWindow/QQuickWindow\n");
    }
}