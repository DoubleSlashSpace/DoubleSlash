package com.doubleslash.client.ui

import android.graphics.Bitmap
import android.graphics.BitmapFactory
import android.util.LruCache
import androidx.compose.foundation.Image
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.layout.widthIn
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.produceState
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.ImageBitmap
import androidx.compose.ui.graphics.asImageBitmap
import androidx.compose.ui.layout.ContentScale
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.compose.ui.window.Dialog
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext
import java.io.File

/** Longest edge kept when decoding a chat image, in pixels. */
private const val PREVIEW_MAX_PX = 1280

/**
 * Decoded chat images, keyed by path.
 *
 * A phone camera photo is ~4000 px wide and the list re-composes on every
 * scroll, so decoding per frame would stutter and churn memory. An eighth of
 * the heap is the usual budget for an image cache of this kind.
 */
private val previewCache = object : LruCache<String, Bitmap>(
    (Runtime.getRuntime().maxMemory() / 8).coerceAtMost(Int.MAX_VALUE.toLong()).toInt(),
) {
    override fun sizeOf(key: String, value: Bitmap): Int = value.byteCount
}

/**
 * Decode `path` down to [PREVIEW_MAX_PX], or null if it is not an image we can
 * read. Blocking: call it off the main thread.
 */
private fun decodePreview(path: String): Bitmap? {
    if (path.isBlank() || !File(path).isFile) return null
    previewCache.get(path)?.let { return it }
    val bounds = BitmapFactory.Options().apply { inJustDecodeBounds = true }
    BitmapFactory.decodeFile(path, bounds)
    val longest = maxOf(bounds.outWidth, bounds.outHeight)
    if (longest <= 0) return null
    var sample = 1
    while (longest / sample > PREVIEW_MAX_PX) sample *= 2
    val decoded = BitmapFactory.decodeFile(
        path,
        BitmapFactory.Options().apply { inSampleSize = sample },
    ) ?: return null
    previewCache.put(path, decoded)
    return decoded
}

@Composable
private fun rememberPreview(path: String): ImageBitmap? {
    val bitmap by produceState<Bitmap?>(initialValue = previewCache.get(path), path) {
        if (value == null) {
            value = withContext(Dispatchers.IO) { decodePreview(path) }
        }
    }
    return remember(bitmap) { bitmap?.asImageBitmap() }
}

/**
 * An attachment inside a chat bubble: the picture itself once it is on this
 * device, otherwise its name.
 *
 * The bubble body is only ever a label ("🖼 name"), so without this an image
 * that had already been downloaded still read as a filename.
 */
@Composable
fun AttachmentContent(
    kind: String,
    name: String,
    path: String,
    sizeStr: String,
    modifier: Modifier = Modifier,
) {
    val preview = if (kind == "image") rememberPreview(path) else null
    var zoomed by remember { mutableStateOf(false) }

    if (preview == null) {
        AttachmentLine(kind, name, sizeStr, modifier)
        return
    }
    Column(modifier) {
        Image(
            bitmap = preview,
            contentDescription = name,
            contentScale = ContentScale.Fit,
            modifier = Modifier
                .heightIn(max = 240.dp)
                .widthIn(max = 260.dp)
                .clip(RoundedCornerShape(12.dp))
                .clickable { zoomed = true },
        )
        if (name.isNotBlank()) {
            Text(
                if (sizeStr.isBlank()) name else "$name · $sizeStr",
                style = MaterialTheme.typography.labelSmall,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
                maxLines = 1,
                overflow = TextOverflow.Ellipsis,
                modifier = Modifier.padding(top = 4.dp).widthIn(max = 260.dp),
            )
        }
    }
    if (zoomed) {
        Dialog(onDismissRequest = { zoomed = false }) {
            Image(
                bitmap = preview,
                contentDescription = name,
                contentScale = ContentScale.Fit,
                modifier = Modifier.fillMaxSize().clickable { zoomed = false },
            )
        }
    }
}

/** A file with no picture to show: its kind, name and size on one line. */
@Composable
private fun AttachmentLine(kind: String, name: String, sizeStr: String, modifier: Modifier) {
    Row(modifier.padding(2.dp), verticalAlignment = Alignment.CenterVertically) {
        Text(
            when (kind) {
                "image" -> "🖼"
                "video" -> "🎬"
                else -> "📎"
            },
        )
        Spacer(Modifier.width(8.dp))
        Column(verticalArrangement = Arrangement.Center) {
            Text(
                name.ifBlank { "Attachment" },
                maxLines = 2,
                overflow = TextOverflow.Ellipsis,
            )
            if (sizeStr.isNotBlank()) {
                Text(
                    sizeStr,
                    style = MaterialTheme.typography.labelSmall,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
            }
        }
    }
}
