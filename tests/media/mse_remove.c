/* Backend acceptance probe, using GStreamer's MSE implementation directly.
 * No container parsing is implemented here. Inputs are Servo's WPT WebM files.
 * Verify removal and refill against the selected native runtime.
 */
#include <gst/gst.h>
#include <gst/mse/mse.h>

static gboolean wait_idle(GstSourceBuffer *buffer) {
    gint64 deadline = g_get_monotonic_time() + 10 * G_TIME_SPAN_SECOND;
    while (gst_source_buffer_get_updating(buffer)) {
        g_main_context_iteration(NULL, FALSE);
        if (g_get_monotonic_time() > deadline) return FALSE;
        g_usleep(1000);
    }
    return TRUE;
}

static gboolean append_file(GstSourceBuffer *buffer, const gchar *path) {
    gchar *bytes = NULL;
    gsize size = 0;
    GError *error = NULL;
    if (!g_file_get_contents(path, &bytes, &size, &error)) {
        g_printerr("%s\n", error->message);
        g_clear_error(&error);
        return FALSE;
    }
    gboolean ok = TRUE;
    for (gsize offset = 0; offset < size; offset += 32768) {
        gsize length = MIN((gsize)32768, size - offset);
        GstBuffer *chunk = gst_buffer_new_allocate(NULL, length, NULL);
        gst_buffer_fill(chunk, 0, bytes + offset, length);
        // append_buffer takes full ownership, including on error.
        ok = gst_source_buffer_append_buffer(buffer, chunk, &error);
        if (!ok || !wait_idle(buffer)) {
            g_printerr("append failed: %s\n", error ? error->message : "timeout");
            g_clear_error(&error);
            ok = FALSE;
            break;
        }
    }
    g_free(bytes);
    return ok;
}

int main(int argc, char **argv) {
    gst_init(&argc, &argv);
    if (argc != 3) {
        g_printerr("usage: mse_remove VIDEO.webm AUDIO.webm\n");
        return 2;
    }
    GstElement *element = gst_element_factory_make("msesrc", NULL);
    if (!element) {
        g_printerr("msesrc plugin missing\n");
        return 2;
    }
    GstMediaSource *source = gst_media_source_new();
    gst_media_source_attach(source, GST_MSE_SRC(element));
    const gchar *types[] = {"video/webm; codecs=\"vp8\"", "audio/webm; codecs=\"vorbis\""};
    gboolean passed = TRUE;
    for (guint i = 0; i < 2; i++) {
        GError *error = NULL;
        GstSourceBuffer *buffer = gst_media_source_add_source_buffer(source, types[i], &error);
        if (!buffer) {
            g_printerr("addSourceBuffer: %s\n", error ? error->message : "failed");
            g_clear_error(&error);
            passed = FALSE;
            break;
        }
        if (!append_file(buffer, argv[i + 1])) {
            passed = FALSE;
            gst_object_unref(buffer);
            break;
        }
        GArray *before = gst_source_buffer_get_buffered(buffer, &error);
        gint64 deadline = g_get_monotonic_time() + 5 * G_TIME_SPAN_SECOND;
        while (before && !before->len && g_get_monotonic_time() < deadline) {
            g_array_unref(before);
            g_main_context_iteration(NULL, FALSE);
            g_usleep(1000);
            before = gst_source_buffer_get_buffered(buffer, &error);
        }
        if (!before || !before->len) {
            g_printerr("No buffered samples before removal\n");
            g_clear_error(&error);
            if (before) g_array_unref(before);
            gst_object_unref(buffer);
            passed = FALSE;
            break;
        }
        GstMediaSourceRange range = g_array_index(before, GstMediaSourceRange, before->len - 1);
        gboolean removed = gst_source_buffer_remove(buffer, 0, range.end + GST_SECOND, &error);
        gboolean idle = wait_idle(buffer);
        GArray *after = error ? NULL : gst_source_buffer_get_buffered(buffer, &error);
        gboolean cleared = removed && idle && after && after->len == 0;
        g_print("%s: before=%u after=%u remove_returned=%d passed=%d\n",
                types[i], before->len, after ? after->len : 0, removed, cleared);
        if (error) g_printerr("remove: %s\n", error->message);
        passed = passed && cleared;
        g_clear_error(&error);
        gboolean refilled = append_file(buffer, argv[i + 1]);
        GArray *refill = gst_source_buffer_get_buffered(buffer, &error);
        refilled = refilled && refill && refill->len > 0;
        g_print("%s: refill=%d\n", types[i], refilled);
        passed = passed && refilled;
        if (refill) g_array_unref(refill);
        g_clear_error(&error);
        g_array_unref(before);
        if (after) g_array_unref(after);
        gst_object_unref(buffer);
    }
    gst_media_source_detach(source);
    gst_object_unref(source);
    gst_object_unref(element);
    return passed ? 0 : 1;
}
