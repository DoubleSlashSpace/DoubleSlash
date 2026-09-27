package com.doubleslash.client.ui

import android.Manifest
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.Button
import androidx.compose.material3.ButtonDefaults
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.Icon
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableLongStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.res.painterResource
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import com.doubleslash.client.CameraCapture
import com.doubleslash.client.ConnectionMode
import com.doubleslash.client.R
import kotlinx.coroutines.delay

/**
 * The live room call, at the foot of every screen.
 *
 * The desktop's `VoiceDock.qml` on a phone: where the call is (tap it to go
 * there), how it is connected and for how long, who in it is sharing video
 * that is not on screen, and the controls. At the bottom rather than the top
 * so the thumb reaches it, and so it sits where the desktop's does — at the
 * foot of the list pane.
 */
@Composable
internal fun VoiceDock(
    roomName: String,
    connectionMode: ConnectionMode,
    muted: Boolean,
    speakerphone: Boolean,
    headsetAttached: Boolean,
    videoActive: Boolean,
    /** People in the call sharing video that we are not watching. */
    unwatchedStreamers: List<String>,
    onOpen: () -> Unit,
    onToggleMute: () -> Unit,
    onToggleSpeaker: () -> Unit,
    onStartVideo: () -> Unit,
    onStopVideo: () -> Unit,
    onWatchStreamers: () -> Unit,
    onLeave: () -> Unit,
) {
    val ds = LocalDsColors.current
    val context = LocalContext.current

    // Counted from when this dock appeared, which is when the call began or
    // the app came back to it; the phone has no session clock of its own.
    val startedAt = remember { System.currentTimeMillis() }
    var now by remember { mutableLongStateOf(startedAt) }
    LaunchedEffect(Unit) {
        while (true) {
            now = System.currentTimeMillis()
            delay(1_000)
        }
    }
    val secs = ((now - startedAt) / 1000).coerceAtLeast(0)
    val duration = "%02d:%02d".format(secs / 60, secs % 60)

    // CameraX must be bound before the core asks for video: the native side
    // waits for a first frame to learn the capture size.
    val requestCamera = rememberExplainedPermission(
        permission = Manifest.permission.CAMERA,
        title = CameraRationaleTitle,
        body = CameraRationaleBody,
        onGranted = {
            CameraCapture.start(context)
            onStartVideo()
        },
    )

    val statusColor = when (connectionMode) {
        ConnectionMode.DIRECT -> ds.online
        ConnectionMode.RELAY -> ds.warn
        ConnectionMode.OFFLINE -> ds.muted
    }
    val modeLabel = when (connectionMode) {
        ConnectionMode.DIRECT -> "Direct"
        ConnectionMode.RELAY -> "Relayed"
        ConnectionMode.OFFLINE -> "Offline"
    }

    Column(
        modifier = Modifier
            .fillMaxWidth()
            .background(ds.bg2),
    ) {
        HorizontalDivider(color = ds.bg0)
        Row(
            verticalAlignment = Alignment.CenterVertically,
            horizontalArrangement = Arrangement.spacedBy(6.dp),
            modifier = Modifier.padding(start = 12.dp, end = 8.dp, top = 6.dp, bottom = 6.dp),
        ) {
            Row(
                verticalAlignment = Alignment.CenterVertically,
                modifier = Modifier
                    .weight(1f)
                    .height(44.dp)
                    .clickable(onClick = onOpen)
                    .semantics { contentDescription = "Open $roomName" },
            ) {
                SignalBars(statusColor)
                Spacer(Modifier.width(8.dp))
                Column {
                    Text(
                        roomName,
                        color = ds.online,
                        fontWeight = FontWeight.Bold,
                        fontSize = 14.sp,
                        maxLines = 1,
                        overflow = TextOverflow.Ellipsis,
                    )
                    Text("$modeLabel · $duration", color = ds.muted, fontSize = 12.sp, maxLines = 1)
                }
            }
            DockButton(
                icon = if (muted) R.drawable.ds_mic_off else R.drawable.ds_mic,
                label = if (muted) "Unmute microphone" else "Mute microphone",
                active = muted,
                activeColor = ds.danger,
                onClick = onToggleMute,
            )
            // A headset takes the audio whatever this says, so the control
            // is shown disabled rather than silently ignored.
            DockButton(
                icon = if (speakerphone && !headsetAttached) R.drawable.ds_speaker else R.drawable.ds_earpiece,
                label = when {
                    headsetAttached -> "Headset"
                    speakerphone -> "Speaker. Tap for earpiece"
                    else -> "Earpiece. Tap for speaker"
                },
                active = speakerphone && !headsetAttached,
                activeColor = ds.accent,
                enabled = !headsetAttached,
                onClick = onToggleSpeaker,
            )
            DockButton(
                icon = if (videoActive) R.drawable.ds_video else R.drawable.ds_video_off,
                label = if (videoActive) "Stop video" else "Share video",
                active = videoActive,
                activeColor = ds.accent,
                onClick = { if (videoActive) onStopVideo() else requestCamera() },
            )
            Box(
                contentAlignment = Alignment.Center,
                modifier = Modifier
                    .size(44.dp)
                    .background(ds.danger)
                    .clickable(onClick = onLeave)
                    .semantics { contentDescription = "Leave voice" },
            ) {
                Icon(painterResource(R.drawable.ds_leave), contentDescription = null, tint = Color.White, modifier = Modifier.size(20.dp))
            }
        }
        if (unwatchedStreamers.isNotEmpty()) {
            Row(
                verticalAlignment = Alignment.CenterVertically,
                modifier = Modifier.padding(start = 14.dp, end = 8.dp, bottom = 8.dp),
            ) {
                Icon(painterResource(R.drawable.ds_video), contentDescription = null, tint = ds.linkPeer, modifier = Modifier.size(14.dp))
                Spacer(Modifier.width(8.dp))
                Text(
                    if (unwatchedStreamers.size == 1) {
                        "${unwatchedStreamers[0]} is sharing video"
                    } else {
                        "${unwatchedStreamers.joinToString(", ")} are sharing video"
                    },
                    color = ds.muted,
                    fontSize = 13.sp,
                    maxLines = 1,
                    overflow = TextOverflow.Ellipsis,
                    modifier = Modifier.weight(1f),
                )
                Button(
                    onClick = onWatchStreamers,
                    shape = RoundedCornerShape(0.dp),
                    contentPadding = PaddingValues(horizontal = 14.dp),
                    colors = ButtonDefaults.buttonColors(containerColor = ds.accent, contentColor = ds.textInv),
                    modifier = Modifier.height(32.dp),
                ) { Text("Watch", fontWeight = FontWeight.Bold, fontSize = 13.sp) }
            }
        }
    }
}

@Composable
private fun SignalBars(color: Color) {
    Row(verticalAlignment = Alignment.Bottom, horizontalArrangement = Arrangement.spacedBy(2.dp)) {
        listOf(5, 9, 13).forEach { h ->
            Box(Modifier.width(3.dp).height(h.dp).background(color))
        }
    }
}

@Composable
private fun DockButton(
    icon: Int,
    label: String,
    active: Boolean,
    activeColor: Color,
    enabled: Boolean = true,
    onClick: () -> Unit,
) {
    val ds = LocalDsColors.current
    Box(
        contentAlignment = Alignment.Center,
        modifier = Modifier
            .size(44.dp)
            .background(if (active) activeColor.copy(alpha = 0.25f) else Color.Transparent)
            .border(1.dp, ds.bg3)
            .clickable(enabled = enabled, onClick = onClick)
            .semantics { contentDescription = label },
    ) {
        Icon(
            painterResource(icon),
            contentDescription = null,
            tint = when {
                !enabled -> ds.muted
                active && activeColor == ds.danger -> ds.danger
                else -> ds.text
            },
            modifier = Modifier.size(20.dp),
        )
    }
}
