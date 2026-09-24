package com.doubleslash.client.ui

import android.graphics.SurfaceTexture
import android.view.Surface
import android.view.TextureView
import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.aspectRatio
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.padding
import androidx.compose.material3.Text
import androidx.compose.material3.MaterialTheme
import androidx.compose.runtime.Composable
import androidx.compose.runtime.key
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.compose.ui.viewinterop.AndroidView
import androidx.compose.ui.window.Dialog
import androidx.compose.ui.window.DialogProperties
import com.doubleslash.client.NativeCore
import com.doubleslash.client.VideoSize

/**
 * A view the native decoder draws [peerId]'s received video into.
 *
 * A `TextureView` rather than a `SurfaceView`: this sits inside the voice
 * rail's scrolling row and under a full-screen dialog, and a `SurfaceView`
 * punches a hole in the window that neither clips nor layers with the rest of
 * Compose. The native side sizes the buffer to the picture, and the view
 * scales it — so callers keep the aspect ratio, see [VideoTile].
 *
 * The surface is attached while it exists and detached before the view lets
 * go of it; frames for a peer with no attached surface are never decoded.
 */
@Composable
fun RemoteVideo(peerId: String, modifier: Modifier = Modifier) {
    // A different peer needs a different view: the attach is bound to the id
    // the surface was created for.
    key(peerId) {
        AndroidView(
            modifier = modifier,
            factory = { context ->
                TextureView(context).apply {
                    surfaceTextureListener = NativeSurfaceBinding(peerId)
                }
            },
        )
    }
}

/** Attaches a texture's surface to the native renderer for its lifetime. */
private class NativeSurfaceBinding(private val peerId: String) : TextureView.SurfaceTextureListener {
    private var surface: Surface? = null
    private var token = 0L

    override fun onSurfaceTextureAvailable(texture: SurfaceTexture, width: Int, height: Int) {
        val created = Surface(texture)
        surface = created
        token = NativeCore.nativeAttachVideoSurface(created, peerId)
    }

    override fun onSurfaceTextureSizeChanged(texture: SurfaceTexture, width: Int, height: Int) = Unit

    override fun onSurfaceTextureDestroyed(texture: SurfaceTexture): Boolean {
        // Detach first: it waits out any draw in progress, after which nothing
        // native touches the surface and it is safe to release.
        NativeCore.nativeDetachVideoSurface(token)
        token = 0L
        surface?.release()
        surface = null
        return true
    }

    override fun onSurfaceTextureUpdated(texture: SurfaceTexture) = Unit
}

/**
 * One peer's video with their name over it, at the picture's own aspect ratio.
 *
 * 16:9 until the first frame reports its real size — what every desktop
 * preset sends — so the common case never jumps.
 */
@Composable
fun VideoTile(
    peerId: String,
    name: String,
    size: VideoSize?,
    stalled: Boolean,
    modifier: Modifier = Modifier,
    onClick: (() -> Unit)? = null,
) {
    val ratio = size?.let { it.width.toFloat() / it.height } ?: (16f / 9f)
    Box(
        modifier
            .aspectRatio(ratio)
            .background(Color.Black)
            .let { if (onClick != null) it.clickable(onClick = onClick) else it },
    ) {
        RemoteVideo(peerId, Modifier.fillMaxSize())
        if (stalled) {
            // The tile keeps its last frame; this says that picture is old.
            Text(
                "Reconnecting…",
                color = Color.White,
                style = MaterialTheme.typography.labelMedium,
                modifier = Modifier
                    .align(Alignment.Center)
                    .background(Color.Black.copy(alpha = 0.6f))
                    .padding(horizontal = 8.dp, vertical = 4.dp),
            )
        }
        Text(
            name,
            color = Color.White,
            style = MaterialTheme.typography.labelSmall,
            maxLines = 1,
            overflow = TextOverflow.Ellipsis,
            modifier = Modifier
                .align(Alignment.BottomStart)
                .background(Color.Black.copy(alpha = 0.5f))
                .padding(horizontal = 6.dp, vertical = 2.dp),
        )
    }
}

/** [peerId]'s video filling the screen. Tap anywhere to close. */
@Composable
fun FullScreenVideo(
    peerId: String,
    name: String,
    size: VideoSize?,
    stalled: Boolean,
    onDismiss: () -> Unit,
) {
    Dialog(
        onDismissRequest = onDismiss,
        properties = DialogProperties(usePlatformDefaultWidth = false),
    ) {
        Box(
            Modifier
                .fillMaxSize()
                .background(Color.Black)
                .clickable(onClick = onDismiss),
            contentAlignment = Alignment.Center,
        ) {
            VideoTile(
                peerId = peerId,
                name = name,
                size = size,
                stalled = stalled,
                onClick = onDismiss,
            )
        }
    }
}
