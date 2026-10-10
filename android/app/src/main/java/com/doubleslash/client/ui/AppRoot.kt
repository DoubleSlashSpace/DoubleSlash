package com.doubleslash.client.ui

import android.Manifest
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.foundation.ExperimentalFoundationApi
import androidx.compose.foundation.Image
import androidx.compose.foundation.background
import androidx.compose.foundation.combinedClickable
import androidx.compose.foundation.clickable
import androidx.compose.foundation.horizontalScroll
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.BoxScope
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.foundation.interaction.DragInteraction
import androidx.compose.foundation.layout.WindowInsets
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.imePadding
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.animation.AnimatedVisibility
import androidx.compose.animation.fadeIn
import androidx.compose.animation.fadeOut
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.lazy.LazyListState
import androidx.compose.foundation.lazy.rememberLazyListState
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.text.BasicTextField
import androidx.compose.ui.graphics.SolidColor
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.foundation.text.KeyboardActions
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.ArrowBack
import androidx.compose.material.icons.automirrored.filled.Send
import androidx.compose.material.icons.filled.KeyboardArrowDown
import androidx.compose.material.icons.filled.AddCircle
import androidx.compose.material.icons.filled.Settings
import androidx.compose.material.icons.filled.Share
import androidx.compose.material.icons.automirrored.filled.List
import androidx.compose.material.icons.filled.Lock
import androidx.compose.material.icons.filled.Person
import androidx.compose.material.icons.filled.Phone
import androidx.compose.material3.NavigationBar
import androidx.compose.material3.NavigationBarItem
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.Button
import androidx.compose.material3.Card
import androidx.compose.material3.Checkbox
import androidx.compose.material3.FilledTonalButton
import androidx.compose.material3.DropdownMenu
import androidx.compose.material3.DropdownMenuItem
import androidx.compose.material3.CardDefaults
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.LinearProgressIndicator
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.ListItem
import androidx.compose.material3.LocalContentColor
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Scaffold
import androidx.compose.material3.SnackbarHost
import androidx.compose.material3.SnackbarHostState
import androidx.compose.material3.RadioButton
import androidx.compose.material3.FilterChip
import androidx.compose.material3.ModalBottomSheet
import androidx.compose.material3.Slider
import androidx.compose.material3.Switch
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.material3.TopAppBar
import androidx.compose.material3.TooltipBox
import androidx.compose.material3.TooltipDefaults
import androidx.compose.material3.PlainTooltip
import androidx.compose.material3.rememberTooltipState
import androidx.compose.runtime.Composable
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.State
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.derivedStateOf
import androidx.compose.runtime.mutableStateMapOf
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.runtime.snapshotFlow
import androidx.compose.runtime.withFrameNanos
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.layout.onGloballyPositioned
import androidx.compose.ui.layout.positionInParent
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.text.input.ImeAction
import androidx.compose.ui.text.input.PasswordVisualTransformation
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.platform.LocalView
import androidx.compose.ui.viewinterop.AndroidView
import androidx.compose.ui.res.painterResource
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import com.doubleslash.client.AppViewModel
import com.doubleslash.client.roomHeadcount
import com.doubleslash.client.roomMembersUnion
import com.doubleslash.client.ownStatusLabel
import com.doubleslash.client.R
import com.doubleslash.client.ChatMessage
import com.doubleslash.client.transferIdFromMessage
import kotlinx.coroutines.flow.drop
import kotlinx.coroutines.flow.first
import kotlinx.coroutines.launch
import com.doubleslash.client.AppState
import com.doubleslash.client.CallPhase
import com.doubleslash.client.CameraCapture
import com.doubleslash.client.CallState
import com.doubleslash.client.AppSettings
import com.doubleslash.client.Legal
import com.doubleslash.client.FileOffer
import com.doubleslash.client.SavedFile
import com.doubleslash.client.SupernodeInfo
import com.doubleslash.client.ConnectionMode
import com.doubleslash.client.HomeTab
import com.doubleslash.client.Room
import com.doubleslash.client.RoomMessage
import com.doubleslash.client.Peer
import com.doubleslash.client.roomSenderName
import com.doubleslash.client.cameraOn
import com.doubleslash.client.videoKey
import com.doubleslash.client.watching
import com.doubleslash.client.inviteContacts
import com.doubleslash.client.trustedPeer
import com.doubleslash.client.TrustOffer
import androidx.compose.ui.draw.alpha
import com.doubleslash.client.VideoSize
import com.doubleslash.client.Screen
import com.doubleslash.client.DateTimeFormats

@Composable
fun AppRoot(viewModel: AppViewModel) {
    val state by viewModel.state.collectAsState()
    val snackbars = remember { SnackbarHostState() }

    // Errors and notices are transient; showing them in a snackbar keeps them
    // out of the layout so a failed send does not shift the message list.
    LaunchedEffect(state.error, state.notice) {
        val message = state.error ?: state.notice ?: return@LaunchedEffect
        snackbars.showSnackbar(message)
        viewModel.dismissError()
    }

    // Hold the screen awake while the camera is live.
    //
    // CameraX unbinds when its lifecycle owner stops, and locking the phone
    // stops it even under `ProcessLifecycleOwner` - so a lock kills the
    // capture mid-call. Android deliberately restricts camera access from the
    // lock screen, so the answer is to not let it lock while streaming rather
    // than to try to keep capturing behind it.
    //
    // Deliberately video-only: an audio call is expected to keep running with
    // the screen off, and pinning the display on for one would waste
    // significant battery for no benefit.
    val view = LocalView.current
    DisposableEffect(state.videoActive) {
        view.keepScreenOn = state.videoActive
        onDispose { view.keepScreenOn = false }
    }

    // One set of member actions for every place a person is listed, so the
    // voice strip and the room's member sheet can never offer different things.
    val memberActions = remember(viewModel) {
        MemberActions(
            onToggleWatch = viewModel::toggleWatchVideo,
            onMessage = viewModel::messageRoomMember,
            onInvite = { roomId, memberId -> viewModel.sendTrustInvite(roomId, memberId) },
        )
    }

    // The Rooms tree's fold state, above the screens so it survives leaving
    // the list and is shared with a room's members sheet.
    var treeFold by remember { mutableStateOf(TreeFold()) }
    // Joining a voice room opens its voice list once. A fold after that stays
    // shut until the user leaves that room and joins again.
    var releasedVoiceKey by remember { mutableStateOf("") }
    val voice = state.voiceRoom
    val voiceKey = if (voice == null) "" else "${voice.supernodeId}/${voice.roomId}"
    LaunchedEffect(voiceKey) {
        if (voiceKey.isEmpty()) {
            releasedVoiceKey = ""
            return@LaunchedEffect
        }
        if (voiceKey == releasedVoiceKey) return@LaunchedEffect
        releasedVoiceKey = voiceKey
        if (voiceKey in treeFold.collapsed) {
            treeFold = treeFold.copy(collapsed = treeFold.collapsed - voiceKey)
        }
    }
    // Joining voice from the tree asks for the microphone first, as the room
    // screen's own Join does.
    var pendingVoiceJoin by remember { mutableStateOf<Room?>(null) }
    // "Invite Contact to Room": the room whose link will be bound to a peer.
    var inviteContactFor by remember { mutableStateOf<Room?>(null) }
    val requestMicForJoin = rememberExplainedPermission(
        permission = Manifest.permission.RECORD_AUDIO,
        title = MicRationaleTitle,
        body = MicRationaleBody,
        onGranted = {
            pendingVoiceJoin?.let(viewModel::joinVoiceIn)
            pendingVoiceJoin = null
        },
    )
    // One set of tree actions for the list and the members sheet; only what a
    // long-press "Report" opens differs between them.
    val treeActionsFor: ((Room) -> Unit) -> RoomTreeActions = { onCreateSubRoom ->
        RoomTreeActions(
            onFold = { treeFold = it },
            onOpenRoom = { room ->
                // Selecting the chat opens this room only while it stays open.
                if (room.key in treeFold.collapsed) {
                    treeFold = treeFold.copy(collapsed = treeFold.collapsed - room.key)
                }
                viewModel.openRoom(room)
            },
            onJoinVoice = { room ->
                pendingVoiceJoin = room
                requestMicForJoin()
            },
            onSetHidden = viewModel::setRoomHidden,
            onCreateSubRoom = onCreateSubRoom,
            members = memberActions,
            onPeerAudio = viewModel::setPeerAudio,
            onTogglePin = viewModel::toggleRoomPin,
            onMoveRoom = viewModel::moveRoom,
            onCopyInvite = viewModel::copyRoomInvite,
            onInviteContact = { inviteContactFor = it },
            onSetMessageAlerts = viewModel::setRoomMessageAlerts,
        )
    }

    Scaffold(
        snackbarHost = { SnackbarHost(snackbars) },
    ) { padding ->
        Column(modifier = Modifier.padding(padding).fillMaxSize()) {
            Surface(modifier = Modifier.weight(1f).fillMaxWidth()) {
            Box(Modifier.fillMaxSize()) {
            when (val screen = state.screen) {
                Screen.Unlock -> UnlockScreen(
                    busy = state.busy,
                    autoUnlocking = state.autoUnlocking,
                    version = viewModel.coreVersion,
                    onUnlock = viewModel::unlock,
                )

                Screen.Terms -> TermsScreen(
                    onAccept = viewModel::acceptTerms,
                    onDecline = viewModel::declineTerms,
                )

                Screen.Home -> HomeScreen(
                    viewModel = viewModel,
                    treeFold = treeFold,
                    treeActionsFor = treeActionsFor,
                )

                is Screen.Portal -> PortalScreen(
                    supernodeId = screen.supernodeId,
                    label = screen.label,
                    myPeerId = state.identity.publicId,
                    core = viewModel.portalCore(),
                    onBack = viewModel::closePortal,
                )

                Screen.Settings -> SettingsScreen(
                    state = state,
                    onBack = viewModel::closeSettings,
                    onSetHandle = viewModel::setHandle,
                    onSetFrontCamera = viewModel::setFrontCamera,
                    onSetVoiceActivation = viewModel::setVoiceActivation,
                    onSetTheme = viewModel::setTheme,
                    onSetFontScale = viewModel::setFontScalePercent,
                    onSetTimeFormat = viewModel::setTimeFormat,
                    onSetAvatarConfig = viewModel::setAvatarConfig,
                    onPreviewAvatar = viewModel::previewAvatar,
                    onPurgeHistory = viewModel::purgeChatHistory,
                    onTrimHistory = viewModel::trimChatHistory,
                    onSetInputGain = viewModel::setInputGain,
                    onSetOutputGain = viewModel::setOutputGain,
                    onSetNoiseStrength = viewModel::setNoiseStrength,
                    onSetVoiceBitrate = viewModel::setVoiceBitrate,
                    onRemoveSupernode = viewModel::removeSupernode,
                    onOpenPortal = viewModel::openPortal,
                    onSetSkin = viewModel::setSkin,
                    onSetStayUnlocked = viewModel::setStayUnlocked,
                    onLock = viewModel::lockAndForget,
                    version = viewModel.coreVersion,
                )

                is Screen.Chat -> ChatScreen(
                    peer = screen.peer,
                    selfChat = screen.peer.peerId == state.identity.peerId,
                    messages = state.messages,
                    onBack = viewModel::closeChat,
                    onSend = viewModel::sendChat,
                    onCall = { viewModel.startCall(screen.peer) },
                    onRetry = { viewModel.retryMessage(it.id) },
                    onDelete = { viewModel.deleteMessage(it.id) },
                    onAcceptInvite = viewModel::acceptInvite,
                    onJoinRoom = viewModel::joinRoomFromInvite,
                    joinableRoomIds = state.rooms.map { it.roomId }.toSet(),
                    transfers = state.transfers,
                    fileRetries = state.fileRetries,
                    onRetryFile = viewModel::retryFile,
                    onSendFile = viewModel::sendFile,
                    timeFormat = state.prefs.timeFormat,
                )

                is Screen.RoomChat -> RoomChatScreen(
                    room = screen.room,
                    messages = state.roomMessages,
                    avatars = state.avatars,
                    peers = state.peers,
                    members = state.roomMembers,
                    chatMembers = state.roomChatMembers,
                    joined = state.roomJoined,
                    voiceActive = state.roomVoiceActive,
                    muted = state.muted,
                    videoActive = state.videoActive,
                    speakerphone = state.speakerphone,
                    headsetAttached = state.headsetAttached,
                    onBack = viewModel::closeRoom,
                    onSend = viewModel::sendRoomChat,
                    onJoinVoice = viewModel::joinRoomVoice,
                    onLeaveVoice = viewModel::leaveRoomVoice,
                    onToggleMute = viewModel::toggleMute,
                    onToggleSpeaker = { viewModel.setSpeakerphone(!state.speakerphone) },
                    // No peer id: the supernode fans room video out to every
                    // participant, rather than it being addressed to one.
                    onToggleVideo = { wanted ->
                        if (wanted) viewModel.startVideo(null) else viewModel.stopVideo(null)
                    },
                    onAcceptInvite = viewModel::acceptInvite,
                    onJoinRoom = viewModel::joinRoomFromInvite,
                    transfers = state.transfers,
                    fileRetries = state.fileRetries,
                    onRetryFile = viewModel::retryFile,
                    onSendFile = viewModel::sendRoomFile,
                    onShare = viewModel::generateRoomInvite,
                    state = state,
                    sheetRows = buildRoomLeaves(
                        room = screen.room,
                        voiceRosters = state.roomVoiceRosters,
                        textRosters = state.roomTextRosters,
                        voiceRoom = state.voiceRoom,
                        fold = treeFold,
                    ),
                    treeFold = treeFold,
                    treeActions = treeActionsFor { },
                )
            }
            // A stream you opened stays on screen when you leave its room.
            FloatingVideo(
                state = state,
                onOpen = { state.voiceRoomEntry()?.let(viewModel::openRoom) },
                modifier = Modifier.align(Alignment.BottomEnd).padding(12.dp),
            )
            }
            }
            // The live call, at the foot of every screen, as the desktop keeps
            // it at the foot of its list pane.
            state.voiceRoom?.let { voice ->
                if (state.screen != Screen.Unlock && state.screen != Screen.Terms) {
                    val myKey = state.identity.publicId.videoKey()
                    val unwatched = state.roomVoiceRosters.roomMembersUnion(voice.roomId)
                        .filter { it.videoKey() != myKey && state.cameraOn(it) && !state.watching(it) }
                    VoiceDock(
                        roomName = voice.roomName.ifBlank { "Voice" },
                        connectionMode = state.connectionMode,
                        muted = state.muted,
                        speakerphone = state.speakerphone,
                        headsetAttached = state.headsetAttached,
                        videoActive = state.videoActive,
                        unwatchedStreamers = unwatched.map { state.peers.roomSenderName(it, "") },
                        onOpen = {
                            val open = (state.screen as? Screen.RoomChat)?.room?.roomId == voice.roomId
                            if (!open) state.voiceRoomEntry()?.let(viewModel::openRoom)
                        },
                        onToggleMute = viewModel::toggleMute,
                        onToggleSpeaker = { viewModel.setSpeakerphone(!state.speakerphone) },
                        // No peer id: the supernode fans room video out.
                        onStartVideo = { viewModel.startVideo(null) },
                        onStopVideo = {
                            viewModel.stopVideo(null)
                            CameraCapture.stop()
                        },
                        onWatchStreamers = { unwatched.forEach(viewModel::toggleWatchVideo) },
                        onLeave = viewModel::leaveRoomVoice,
                    )
                }
            }
        }
    }

    state.inviteUrl?.let { url ->
        InviteDialog(url = url, onDismiss = viewModel::dismissInvite)
    }

    inviteContactFor?.let { room ->
        InviteContactDialog(
            roomName = room.roomName.ifBlank { room.roomId.take(12) },
            contacts = state.peers.inviteContacts(),
            onPick = { peer ->
                inviteContactFor = null
                viewModel.copyRoomInviteFor(room, peer)
            },
            onDismiss = { inviteContactFor = null },
        )
    }

    state.trustOffers.firstOrNull()?.let { offer ->
        TrustOfferDialog(
            offer = offer,
            roomName = state.rooms.firstOrNull { it.roomId == offer.roomId }
                ?.roomName?.ifBlank { null } ?: "a room",
            avatar = state.avatars[offer.senderId],
            waiting = state.trustOffers.size - 1,
            onAnswer = viewModel::answerTrustOffer,
        )
    }

    // A file offer is a question, not a notification: nothing is transferred
    // until it is answered, so it interrupts rather than waiting in a list.
    state.fileOffer?.let { offer ->
        FileOfferDialog(
            offer = offer,
            onAccept = viewModel::acceptFileOffer,
            onReject = viewModel::rejectFileOffer,
        )
    }

    state.savedFile?.let { saved ->
        SaveFilePrompt(
            saved = saved,
            onSave = viewModel::exportSavedFile,
            onDismiss = viewModel::dismissSavedFile,
        )
    }

    state.call?.let { call ->
        CallOverlay(
            call = call,
            videoActive = state.videoActive,
            speakerphone = state.speakerphone,
            headsetAttached = state.headsetAttached,
            peerCameraOn = state.cameraOn(call.peerId),
            watchingPeer = state.watching(call.peerId),
            // Keyed by the id frames arrive under, which on a direct call is
            // the hex peer id rather than the public id.
            peerVideoSize = state.videoSizes[call.peerId.videoKey()],
            peerVideoStalled = call.peerId.videoKey() in state.stalledVideo,
            onToggleWatch = { viewModel.toggleWatchVideo(call.peerId) },
            onAccept = viewModel::acceptCall,
            onReject = viewModel::rejectCall,
            onEnd = viewModel::endCall,
            onToggleMute = viewModel::toggleMute,
            onToggleSpeaker = { viewModel.setSpeakerphone(!state.speakerphone) },
            onToggleVideo = { wanted ->
                if (wanted) viewModel.startVideo(call.peerId) else viewModel.stopVideo(call.peerId)
            },
        )
    }

    if (state.screen is Screen.Home) {
        NotificationPermissionPrompt(AppSettings(LocalContext.current))
    }
}

/**
 * Incoming calls take over the screen; calls already in progress sit in a bar
 * so the rest of the app stays usable during them.
 */
@Composable
private fun CallOverlay(
    call: CallState,
    videoActive: Boolean,
    speakerphone: Boolean,
    headsetAttached: Boolean,
    peerCameraOn: Boolean,
    watchingPeer: Boolean,
    peerVideoSize: VideoSize?,
    peerVideoStalled: Boolean,
    onToggleWatch: () -> Unit,
    onAccept: () -> Unit,
    onReject: () -> Unit,
    onEnd: () -> Unit,
    onToggleMute: () -> Unit,
    onToggleSpeaker: () -> Unit,
    onToggleVideo: (Boolean) -> Unit,
) {
    val context = LocalContext.current
    var peerMenuOpen by remember { mutableStateOf(false) }
    var fullScreen by remember { mutableStateOf(false) }

    // CameraX has to be bound before the core asks for video: the native side
    // waits a few seconds for a first frame to learn the capture size, so
    // binding afterwards would race that timeout. The in-app explanation
    // runs first: video continues with the screen off.
    val requestCamera = rememberExplainedPermission(
        permission = Manifest.permission.CAMERA,
        title = CameraRationaleTitle,
        body = CameraRationaleBody,
        onGranted = {
            CameraCapture.start(context)
            onToggleVideo(true)
        },
    )
    val requestMicToAnswer = rememberExplainedPermission(
        permission = Manifest.permission.RECORD_AUDIO,
        title = MicRationaleTitle,
        body = MicRationaleBody,
        onGranted = onAccept,
    )
    if (call.phase == CallPhase.INCOMING) {
        AlertDialog(
            onDismissRequest = { /* a ringing call needs an explicit answer */ },
            title = { Text("Incoming call") },
            text = { Text(call.peerLabel) },
            confirmButton = { TextButton(onClick = { requestMicToAnswer() }) { Text("Answer") } },
            dismissButton = { TextButton(onClick = onReject) { Text("Decline") } },
        )
        return
    }

    val showVideo = watchingPeer && peerCameraOn
    Box(Modifier.fillMaxSize(), contentAlignment = Alignment.BottomCenter) {
        Column(horizontalAlignment = Alignment.CenterHorizontally) {
            if (showVideo) {
                VideoTile(
                    peerId = call.peerId,
                    name = call.peerLabel,
                    size = peerVideoSize,
                    stalled = peerVideoStalled,
                    modifier = Modifier.padding(horizontal = 12.dp).height(CALL_VIDEO_HEIGHT),
                    onClick = { fullScreen = true },
                )
            }
            Card(
                modifier = Modifier.fillMaxWidth().padding(12.dp),
                colors = CardDefaults.cardColors(
                    containerColor = MaterialTheme.colorScheme.primaryContainer,
                ),
            ) {
                Row(
                    modifier = Modifier.fillMaxWidth().padding(12.dp),
                    verticalAlignment = Alignment.CenterVertically,
                ) {
                    // The peer's name opens their menu, as a face in the voice rail
                    // does: video is watched on request, not pushed at the user.
                    Box(Modifier.weight(1f)) {
                        Column(Modifier.clickable { peerMenuOpen = true }) {
                            Text(call.peerLabel, style = MaterialTheme.typography.titleSmall)
                            Text(
                                when {
                                    call.phase == CallPhase.OUTGOING -> "Calling..."
                                    peerCameraOn && !watchingPeer -> "In call · camera on"
                                    else -> "In call"
                                },
                                style = MaterialTheme.typography.labelSmall,
                            )
                        }
                        DropdownMenu(expanded = peerMenuOpen, onDismissRequest = { peerMenuOpen = false }) {
                            DropdownMenuItem(
                                text = {
                                    Text(
                                        when {
                                            watchingPeer -> "Stop watching"
                                            peerCameraOn -> "Watch video"
                                            else -> "Camera is off"
                                        }
                                    )
                                },
                                enabled = watchingPeer || peerCameraOn,
                                onClick = {
                                    peerMenuOpen = false
                                    onToggleWatch()
                                },
                            )
                        }
                    }
                    TextButton(onClick = onToggleMute) {
                        Text(if (call.muted) "Unmute" else "Mute")
                    }
                    // A headset takes the audio regardless of this preference, so
                    // the control is disabled rather than silently ignored.
                    TextButton(onClick = onToggleSpeaker, enabled = !headsetAttached) {
                        Text(
                            when {
                                headsetAttached -> "Headset"
                                speakerphone -> "Speaker"
                                else -> "Earpiece"
                            }
                        )
                    }
                    TextButton(
                        onClick = {
                            if (videoActive) {
                                onToggleVideo(false)
                                CameraCapture.stop()
                            } else {
                                requestCamera()
                            }
                        },
                    ) {
                        Text(if (videoActive) "Stop video" else "Video")
                    }
                    TextButton(onClick = onEnd) { Text("End") }
                }
            }
        }
    }

    if (fullScreen && showVideo) {
        FullScreenVideo(
            peerId = call.peerId,
            name = call.peerLabel,
            size = peerVideoSize,
            stalled = peerVideoStalled,
            onDismiss = { fullScreen = false },
        )
    }
}

/** Larger than a rail tile: in a call this is the one picture there is. */
private val CALL_VIDEO_HEIGHT = 220.dp

// ── Unlock ─────────────────────────────────────────────────────────────────

@Composable
private fun UnlockScreen(
    busy: Boolean,
    autoUnlocking: Boolean,
    version: String,
    onUnlock: (String, Boolean, android.net.Uri?) -> Unit,
) {
    var passphrase by remember { mutableStateOf("") }
    var stayUnlocked by remember { mutableStateOf(false) }
    var keyfile by remember { mutableStateOf<android.net.Uri?>(null) }

    val pickKeyfile = rememberLauncherForActivityResult(
        ActivityResultContracts.OpenDocument(),
    ) { uri -> keyfile = uri }

    // A stored key is being tried: asking for a passphrase we are about to not
    // need would flash a prompt on every launch, which is the thing the user
    // turned this on to avoid.
    if (autoUnlocking) {
        Column(
            modifier = Modifier.fillMaxSize().padding(24.dp),
            verticalArrangement = Arrangement.Center,
            horizontalAlignment = Alignment.CenterHorizontally,
        ) {
            Image(
                painter = painterResource(R.drawable.ic_logo_full),
                contentDescription = null,
                modifier = Modifier.width(260.dp).height(48.dp),
            )
            Spacer(Modifier.height(24.dp))
            CircularProgressIndicator(strokeWidth = 2.dp)
        }
        return
    }

    Column(
        modifier = Modifier.fillMaxSize().padding(24.dp),
        horizontalAlignment = Alignment.CenterHorizontally,
    ) {
        // Optical centre rather than Arrangement.Center: the block is roughly
        // 350dp in an 850dp viewport, and splitting that slack evenly leaves a
        // quarter of the screen empty above the mark. Weighting the gap 1:2
        // lifts it to where the eye expects a sign-in screen to sit, and keeps
        // the field high enough that the IME does not shove the layout when it
        // opens.
        Spacer(Modifier.weight(1f))

        Image(
            painter = painterResource(R.drawable.ic_logo_full),
            contentDescription = null,
            modifier = Modifier.width(260.dp).height(48.dp),
        )
        Spacer(Modifier.height(16.dp))

        Text(
            "Your identity never leaves this device.",
            style = MaterialTheme.typography.bodyMedium,
            color = MaterialTheme.colorScheme.onSurfaceVariant,
        )

        Spacer(Modifier.height(32.dp))

        OutlinedTextField(
            value = passphrase,
            onValueChange = { passphrase = it },
            label = { Text("Passphrase") },
            supportingText = { Text("Leave empty for an unencrypted identity.") },
            singleLine = true,
            visualTransformation = PasswordVisualTransformation(),
            keyboardOptions = KeyboardOptions(imeAction = ImeAction.Go),
            keyboardActions = KeyboardActions(
                onGo = { onUnlock(passphrase, stayUnlocked, keyfile) },
            ),
            enabled = !busy,
            modifier = Modifier.fillMaxWidth(),
        )

        Spacer(Modifier.height(8.dp))

        // A keyfile can stand in for a passphrase or strengthen one: the core
        // hashes the file and appends it to the typed text. Matching the
        // desktop matters here - an identity created with both only opens
        // with both.
        Row(
            modifier = Modifier.fillMaxWidth(),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            TextButton(
                onClick = { pickKeyfile.launch(arrayOf("*/*")) },
                enabled = !busy,
            ) { Text(if (keyfile == null) "Use a keyfile" else "Change keyfile") }

            if (keyfile != null) {
                TextButton(onClick = { keyfile = null }, enabled = !busy) { Text("Clear") }
            }
        }
        if (keyfile != null) {
            Text(
                keyfile?.lastPathSegment ?: "keyfile selected",
                style = MaterialTheme.typography.labelSmall,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
                maxLines = 1,
                overflow = TextOverflow.Ellipsis,
                modifier = Modifier.fillMaxWidth(),
            )
        }

        Spacer(Modifier.height(8.dp))

        // ── Optional auto-unlock ──────────────────────────────────────────
        //
        // Off unless the user turns it on, and the caption states both sides
        // rather than selling the convenience: the cost - anyone who can use
        // this phone can open the identity - is the part that is easy to miss.
        Row(
            modifier = Modifier
                .fillMaxWidth()
                .clickable(enabled = !busy) { stayUnlocked = !stayUnlocked },
            verticalAlignment = Alignment.CenterVertically,
        ) {
            Checkbox(
                checked = stayUnlocked,
                onCheckedChange = { stayUnlocked = it },
                enabled = !busy,
            )
            Column(Modifier.padding(start = 4.dp)) {
                Text("Stay unlocked on this device", style = MaterialTheme.typography.bodyMedium)
                Text(
                    if (stayUnlocked) {
                        "DoubleSlash will open without this passphrase. Your key is kept in " +
                            "the Android Keystore, so anyone who can use this phone can open " +
                            "your identity. Your passphrase itself is never stored."
                    } else {
                        "You will type this passphrase every launch. Your identity stays " +
                            "unreadable to anyone who gets the phone's files."
                    },
                    style = MaterialTheme.typography.labelSmall,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
            }
        }

        Spacer(Modifier.height(16.dp))

        Button(
            onClick = { onUnlock(passphrase, stayUnlocked, keyfile) },
            enabled = !busy,
            modifier = Modifier.fillMaxWidth(),
        ) {
            if (busy) {
                CircularProgressIndicator(
                    modifier = Modifier.size(18.dp),
                    strokeWidth = 2.dp,
                    color = MaterialTheme.colorScheme.onPrimary,
                )
            } else {
                Text("Unlock")
            }
        }

        Spacer(Modifier.height(16.dp))
        BackupButton(unlocked = false, enabled = !busy)
        val context = LocalContext.current
        TextButton(onClick = { Legal.openUrl(context, Legal.PRIVACY_URL) }) {
            Text("Privacy policy")
        }

        Spacer(Modifier.height(24.dp))
        Text(
            "core $version",
            style = MaterialTheme.typography.labelSmall,
            color = MaterialTheme.colorScheme.onSurfaceVariant,
        )

        Spacer(Modifier.weight(2f))
    }
}

// ── Home ───────────────────────────────────────────────────────────────────

/** SVG-derived vector icons shared with the desktop controls. */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
private fun ListActionIcon(
    drawable: Int,
    label: String,
    enabled: Boolean = true,
    selected: Boolean = false,
    count: Int = 0,
    onClick: () -> Unit,
) {
    TooltipBox(
        positionProvider = TooltipDefaults.rememberPlainTooltipPositionProvider(),
        tooltip = { PlainTooltip { Text(label) } },
        state = rememberTooltipState(),
    ) {
        Box {
            IconButton(onClick = onClick, enabled = enabled, modifier = Modifier.size(32.dp)) {
                Icon(
                    painterResource(drawable), contentDescription = label,
                    modifier = Modifier.size(18.dp),
                    tint = if (selected) MaterialTheme.colorScheme.primary else LocalContentColor.current,
                )
            }
            if (count > 0) Text(
                count.toString(), modifier = Modifier.align(Alignment.TopEnd),
                style = MaterialTheme.typography.labelSmall,
            )
        }
    }
}

@OptIn(ExperimentalMaterial3Api::class)
@Composable
private fun HomeScreen(
    viewModel: AppViewModel,
    treeFold: TreeFold,
    treeActionsFor: ((Room) -> Unit) -> RoomTreeActions,
) {
    val state by viewModel.state.collectAsState()
    var showAccept by remember { mutableStateOf(false) }
    var showSortMenu by remember { mutableStateOf(false) }
    var peerSortMode by rememberSaveable { mutableStateOf("online") }
    var inviteText by rememberSaveable { mutableStateOf("") }
    var confirmRemovePeer by remember { mutableStateOf<Peer?>(null) }
    var reportPeer by remember { mutableStateOf<Peer?>(null) }
    var showCreateRoom by remember { mutableStateOf(false) }
    // A room long-pressed for "Create room inside": the dialog opens with it
    // already chosen as the parent.
    var subRoomParent by remember { mutableStateOf<Room?>(null) }

    Column(Modifier.fillMaxSize()) {
        TopAppBar(
            // The Scaffold above already pays the status-bar inset for this
            // content, and an M3 top bar applies its own by default - which
            // insets the bar twice and leaves a status-bar-height band of dead
            // space above the title. These bars live inside the Scaffold body
            // rather than its topBar slot, so the inset is not theirs to add.
            windowInsets = WindowInsets(0, 0, 0, 0),
            navigationIcon = {
                Box(Modifier.size(48.dp), contentAlignment = Alignment.Center) {
                    Image(
                        painter = painterResource(R.drawable.ic_logo),
                        contentDescription = "DoubleSlash",
                        modifier = Modifier.width(34.dp).height(16.dp),
                    )
                }
            },
            // Peers | Rooms beside the logo: the one switch between the two
            // lists, where the desktop's title bar has it too. There is no tab
            // bar at the bottom any more.
            title = {
                ListToggle(
                    rooms = state.tab == HomeTab.ROOMS,
                    onSelect = { rooms ->
                        viewModel.selectTab(if (rooms) HomeTab.ROOMS else HomeTab.PEERS)
                    },
                )
            },
            actions = {
                // Your avatar opens Settings on Identity, as the desktop's
                // title-bar avatar does. The logo's old app menu lives there.
                IconButton(
                    onClick = viewModel::openSettings,
                    modifier = Modifier.semantics { contentDescription = "Your identity" },
                ) {
                    val me = state.avatars[state.identity.peerId]
                    if (me != null) {
                        Box(Modifier.size(30.dp).clip(CircleShape)) { Avatar(me, Modifier.size(30.dp)) }
                    } else {
                        Icon(Icons.Filled.Person, contentDescription = null)
                    }
                }
            },
        )

        // Compact invite controls stay available on both lists.
        Row(
            Modifier.fillMaxWidth().padding(horizontal = 12.dp, vertical = 2.dp),
            horizontalArrangement = Arrangement.spacedBy(4.dp),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            BasicTextField(
                value = inviteText,
                onValueChange = { inviteText = it },
                singleLine = true,
                textStyle = MaterialTheme.typography.bodySmall.copy(color = MaterialTheme.colorScheme.onSurface),
                cursorBrush = SolidColor(MaterialTheme.colorScheme.primary),
                keyboardOptions = KeyboardOptions(imeAction = ImeAction.Go),
                keyboardActions = KeyboardActions(onGo = {
                    if (inviteText.isNotBlank()) {
                        viewModel.acceptInvite(inviteText)
                        inviteText = ""
                    }
                }),
                modifier = Modifier.weight(1f).height(32.dp)
                    .background(MaterialTheme.colorScheme.surfaceVariant, RoundedCornerShape(6.dp))
                    .semantics { contentDescription = "Invite link or peer ID" },
                decorationBox = { field ->
                    Box(Modifier.padding(horizontal = 8.dp), contentAlignment = Alignment.CenterStart) {
                        if (inviteText.isEmpty()) Text(
                            "Paste invite\u2026", style = MaterialTheme.typography.bodySmall,
                            color = MaterialTheme.colorScheme.onSurfaceVariant,
                        )
                        field()
                    }
                },
            )
            ListActionIcon(R.drawable.ic_invite_submit, "Accept invite", enabled = inviteText.isNotBlank()) {
                viewModel.acceptInvite(inviteText)
                inviteText = ""
            }
            ListActionIcon(R.drawable.ic_invite, "Copy invite", enabled = !state.busy) {
                viewModel.generateInvite(copyToClipboard = true)
            }
        }

        ConnectionBanner(state.connectionMode)
        Row(
            Modifier.fillMaxWidth().padding(horizontal = 12.dp, vertical = 2.dp),
            horizontalArrangement = Arrangement.spacedBy(4.dp),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            val rooms = state.tab == HomeTab.ROOMS
            val hiddenCount = if (rooms) state.rooms.count { it.hidden } else state.peers.count { it.blocked }
            val showing = if (rooms) state.showHiddenRooms else state.showBlockedPeers
            ListActionIcon(
                if (rooms) R.drawable.ic_list_plus else R.drawable.ic_invite,
                if (rooms) "Create Room" else "Create an Invite",
                enabled = if (rooms) state.supernodes.isNotEmpty() else !state.busy,
            ) {
                if (rooms) showCreateRoom = true else viewModel.generateInvite(copyToClipboard = true)
            }
            ListActionIcon(
                R.drawable.ic_list_eye,
                if (showing) "Hide $hiddenCount hidden" else "Show $hiddenCount hidden",
                enabled = hiddenCount > 0,
                selected = showing,
                count = hiddenCount,
            ) {
                if (rooms) viewModel.toggleShowHiddenRooms() else viewModel.toggleShowBlockedPeers()
            }
            Box {
                ListActionIcon(R.drawable.ic_list_sort, if (rooms) "Sort Rooms" else "Sort Peers") {
                    showSortMenu = true
                }
                DropdownMenu(expanded = showSortMenu, onDismissRequest = { showSortMenu = false }) {
                    val options = if (rooms) ROOM_SORT_OPTIONS else listOf(
                        "name_asc" to "Name (A\u2013Z)", "name_desc" to "Name (Z\u2013A)", "online" to "Online first",
                    )
                    options.forEach { (mode, label) ->
                        val selected = mode == if (rooms) state.prefs.roomListOrder.mode else peerSortMode
                        DropdownMenuItem(
                            text = { Text(if (selected) "\u2713  $label" else label) },
                            onClick = {
                                showSortMenu = false
                                if (rooms) viewModel.setRoomSortMode(mode) else peerSortMode = mode
                            },
                        )
                    }
                }
            }
            Spacer(Modifier.weight(1f))
            ListActionIcon(R.drawable.ic_list_refresh, "Refresh") {
                if (rooms) viewModel.refreshRooms() else viewModel.refreshPeers()
            }
        }


        if (state.tab == HomeTab.PEERS) {
            OwnPeerRow(state = state, onClick = viewModel::openSelfChat)
            HorizontalDivider()
        }

        Box(Modifier.weight(1f)) {
            when (state.tab) {
                HomeTab.PEERS -> PeersList(
                    state = state,
                    sortMode = peerSortMode,
                    onOpenPeer = viewModel::openChat,
                    onCreateInvite = { viewModel.generateInvite() },
                    onAcceptInvite = { showAccept = true },
                    onSetBlocked = { peer, blocked ->
                        viewModel.setPeerBlocked(peer.peerId, blocked)
                    },
                    onRemove = { peer -> confirmRemovePeer = peer },
                    onReport = { peer -> reportPeer = peer },
                )

                HomeTab.ROOMS -> {
                    // Hidden is per-profile local state, so the desktop's
                    // choices arrive with the room list and are honoured here.
                    val visible = state.rooms.filter { state.showHiddenRooms || !it.hidden }
                    if (visible.isEmpty()) {
                        Column(
                            modifier = Modifier.fillMaxSize().padding(32.dp),
                            verticalArrangement = Arrangement.Center,
                            horizontalAlignment = Alignment.CenterHorizontally,
                        ) {
                            Text(
                                if (state.rooms.isEmpty()) "No rooms yet" else "All rooms are hidden",
                                style = MaterialTheme.typography.titleMedium,
                            )
                            Spacer(Modifier.height(8.dp))
                            Text(
                                if (state.rooms.isEmpty()) {
                                    "Rooms you create or are invited to appear here."
                                } else {
                                    "Use Show hidden in the list header to show them."
                                },
                                style = MaterialTheme.typography.bodyMedium,
                                color = MaterialTheme.colorScheme.onSurfaceVariant,
                            )
                        }
                    } else {
                        val treeActions = treeActionsFor { room -> subRoomParent = room }
                        val rows = buildRoomTree(
                            rooms = visible,
                            voiceRosters = state.roomVoiceRosters,
                            textRosters = state.roomTextRosters,
                            reading = (state.screen as? Screen.RoomChat)?.room,
                            voiceRoom = state.voiceRoom,
                            fold = treeFold,
                            order = state.prefs.roomListOrder,
                        )
                        LazyColumn(Modifier.fillMaxSize()) {
                            roomTreeItems(rows, state, treeFold, treeActions)
                        }
                    }
                }
            }
        }
    }

    if (showAccept) {
        AcceptInviteDialog(
            onDismiss = { showAccept = false },
            onAccept = {
                viewModel.acceptInvite(it)
                showAccept = false
            },
        )
    }

    confirmRemovePeer?.let { peer ->
        RemovePeerDialog(
            peer = peer,
            onDismiss = { confirmRemovePeer = null },
            onConfirm = {
                confirmRemovePeer = null
                viewModel.removePeer(peer.peerId)
            },
        )
    }

    reportPeer?.let { peer ->
        ReportDialog(
            kind = "peer",
            targetId = peer.peerId,
            targetLabel = peer.label,
            onBlock = { viewModel.setPeerBlocked(peer.peerId, true) },
            onDismiss = { reportPeer = null },
        )
    }

    if (showCreateRoom) {
        CreateRoomDialog(
            supernodes = state.supernodes,
            // Parents follow the Rooms list: a hidden room is not offered to
            // nest under unless hidden rooms are being shown.
            rooms = state.rooms.filter { state.showHiddenRooms || !it.hidden },
            onDismiss = { showCreateRoom = false },
            onCreate = { supernodeId, name, isPrivate, parentRoomId ->
                showCreateRoom = false
                viewModel.createRoom(supernodeId, name, isPrivate, parentRoomId)
            },
        )
    }

    subRoomParent?.let { parent ->
        CreateRoomDialog(
            supernodes = state.supernodes,
            rooms = emptyList(),
            fixedParent = parent,
            onDismiss = { subRoomParent = null },
            onCreate = { supernodeId, name, isPrivate, parentRoomId ->
                subRoomParent = null
                viewModel.createRoom(supernodeId, name, isPrivate, parentRoomId)
            },
        )
    }

}

@Composable
private fun PeersList(
    state: AppState,
    sortMode: String,
    onOpenPeer: (Peer) -> Unit,
    onCreateInvite: () -> Unit,
    onAcceptInvite: () -> Unit,
    onSetBlocked: (Peer, Boolean) -> Unit,
    onRemove: (Peer) -> Unit,
    onReport: (Peer) -> Unit,
) {
    if (state.peers.isEmpty()) {
        EmptyPeers(onCreateInvite = onCreateInvite, onAcceptInvite = onAcceptInvite)
        return
    }
    // Blocked peers are the list's hidden ones, as on the desktop: out of the
    // way until the list header's "Show hidden".
    val byName = compareBy<Peer> { it.label.lowercase(java.util.Locale.ROOT) }.thenBy { it.peerId }
    val order = when (sortMode) {
        "name_desc" -> byName.reversed()
        "name_asc" -> byName
        else -> compareBy<Peer> { it.peerId !in state.onlinePeers }.then(byName)
    }
    val shown = state.peers.filter { state.showBlockedPeers || !it.blocked }.sortedWith(order)
    if (shown.isEmpty()) {
        Column(
            modifier = Modifier.fillMaxSize().padding(32.dp),
            verticalArrangement = Arrangement.Center,
            horizontalAlignment = Alignment.CenterHorizontally,
        ) {
            Text("All peers are blocked", style = MaterialTheme.typography.titleMedium)
            Spacer(Modifier.height(8.dp))
            Text(
                "Use Show hidden in the list header to show them.",
                style = MaterialTheme.typography.bodyMedium,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
            )
        }
        return
    }

    LazyColumn(Modifier.fillMaxSize()) {
        items(shown, key = { it.peerId }) { peer ->
            PeerRow(
                peer = peer,
                online = peer.peerId in state.onlinePeers,
                avatar = state.avatars[peer.peerId],
                onClick = { onOpenPeer(peer) },
                onSetBlocked = { blocked -> onSetBlocked(peer, blocked) },
                onRemove = { onRemove(peer) },
                onReport = { onReport(peer) },
            )
            HorizontalDivider()
        }
        item {
            TextButton(
                onClick = onAcceptInvite,
                modifier = Modifier.fillMaxWidth().padding(16.dp),
            ) { Text("Accept an invite") }
        }
    }
}

/**
 * Our own row, pinned above the peers and outside their online/offline order.
 * Opening it is self-chat, which reaches every device signed in as us.
 */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
private fun OwnPeerRow(state: AppState, onClick: () -> Unit) {
    val online = state.connectedSupernodes.isNotEmpty()
    val name = state.identity.handle.ifEmpty { "Me" }
    TooltipBox(
        positionProvider = TooltipDefaults.rememberPlainTooltipPositionProvider(),
        tooltip = { PlainTooltip { Text("Message myself") } },
        state = rememberTooltipState(),
    ) {
        ListItem(
            headlineContent = { Text("$name (you)") },
            supportingContent = {
                Text(
                    ownStatusLabel(online, state.ownDevicesOnline),
                    maxLines = 1,
                    overflow = TextOverflow.Ellipsis,
                    style = MaterialTheme.typography.bodySmall,
                )
            },
            leadingContent = {
                Row(verticalAlignment = Alignment.CenterVertically) {
                    Box(
                        Modifier
                            .size(10.dp)
                            .clip(CircleShape)
                            .background(
                                if (online) Color(0xFF16A34A) else MaterialTheme.colorScheme.outlineVariant,
                            ),
                    )
                    Spacer(Modifier.width(10.dp))
                    val avatar = state.avatars[state.identity.peerId]
                    if (avatar != null) {
                        Avatar(avatar, Modifier.size(36.dp))
                    } else {
                        Box(
                            Modifier
                                .size(36.dp)
                                .clip(RoundedCornerShape(percent = 18))
                                .background(MaterialTheme.colorScheme.surfaceVariant),
                        )
                    }
                }
            },
            modifier = Modifier.clickable(onClickLabel = "Message myself", onClick = onClick),
        )
    }
}

@Composable
private fun ConnectionBanner(mode: ConnectionMode) {
    val (label, color) = when (mode) {
        ConnectionMode.DIRECT -> "Direct" to Color(0xFF16A34A)
        ConnectionMode.RELAY -> "Relayed" to Color(0xFFCA8A04)
        ConnectionMode.OFFLINE -> "Offline" to MaterialTheme.colorScheme.onSurfaceVariant
    }

    Row(
        modifier = Modifier.fillMaxWidth().padding(horizontal = 16.dp, vertical = 8.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Box(Modifier.size(8.dp).clip(CircleShape).background(color))
        Spacer(Modifier.width(8.dp))
        Text(label, style = MaterialTheme.typography.labelMedium, color = color)
    }
}

@OptIn(ExperimentalFoundationApi::class)
@Composable
private fun PeerRow(
    peer: Peer,
    online: Boolean,
    avatar: AvatarArt?,
    onClick: () -> Unit,
    onSetBlocked: (Boolean) -> Unit,
    onRemove: () -> Unit,
    onReport: () -> Unit,
) {
    var menuOpen by remember { mutableStateOf(false) }
    val clipboard = androidx.compose.ui.platform.LocalClipboardManager.current

    Box {
    ListItem(
        headlineContent = {
            Row(verticalAlignment = Alignment.CenterVertically) {
                Text(peer.label)
                // Blocked is otherwise invisible: the row looks identical to a
                // peer who simply is not online, which is the wrong story.
                if (peer.blocked) {
                    Spacer(Modifier.width(6.dp))
                    Text(
                        "blocked",
                        style = MaterialTheme.typography.labelSmall,
                        color = MaterialTheme.colorScheme.error,
                    )
                }
            }
        },
        supportingContent = {
            Text(
                peer.peerId,
                maxLines = 1,
                overflow = TextOverflow.Ellipsis,
                style = MaterialTheme.typography.bodySmall,
            )
        },
        leadingContent = {
            Row(verticalAlignment = Alignment.CenterVertically) {
                Box(
                    Modifier
                        .size(10.dp)
                        .clip(CircleShape)
                        .background(
                            if (online) {
                                Color(0xFF16A34A)
                            } else {
                                MaterialTheme.colorScheme.outlineVariant
                            },
                        ),
                )
                Spacer(Modifier.width(10.dp))
                // Until the identicon arrives the row keeps its shape with a
                // blank of the same size, so the list does not jump.
                if (avatar != null) {
                    Avatar(avatar, Modifier.size(36.dp))
                } else {
                    Box(
                        Modifier
                            .size(36.dp)
                            .clip(RoundedCornerShape(percent = 18))
                            .background(MaterialTheme.colorScheme.surfaceVariant),
                    )
                }
            }
        },
        // Long-press for the peer actions, the same gesture rooms already use
        // for hide/unhide - one idiom for "more, on this row".
        modifier = Modifier.combinedClickable(
            onClick = onClick,
            onLongClick = { menuOpen = true },
        ),
    )

    DropdownMenu(expanded = menuOpen, onDismissRequest = { menuOpen = false }) {
        DropdownMenuItem(
            text = { Text("Copy peer ID") },
            onClick = {
                clipboard.setText(androidx.compose.ui.text.AnnotatedString(peer.peerId))
                menuOpen = false
            },
        )
        DropdownMenuItem(
            text = { Text(if (peer.blocked) "Unblock" else "Block") },
            onClick = {
                onSetBlocked(!peer.blocked)
                menuOpen = false
            },
        )
        DropdownMenuItem(
            text = { Text("Report") },
            onClick = {
                onReport()
                menuOpen = false
            },
        )
        HorizontalDivider()
        DropdownMenuItem(
            text = { Text("Remove peer", color = MaterialTheme.colorScheme.error) },
            onClick = {
                onRemove()
                menuOpen = false
            },
        )
    }
    }
}

/**
 * Create a room on one of the supernodes we know.
 *
 * The host picker only appears when there is a choice to make — with one
 * supernode, asking which is noise.
 */
@Composable
private fun CreateRoomDialog(
    supernodes: List<Peer>,
    rooms: List<Room>,
    onDismiss: () -> Unit,
    onCreate: (String, String, Boolean, String) -> Unit,
    /**
     * Create inside this room: from a room's own menu, so the parent and its
     * host are already known and neither picker is shown.
     */
    fixedParent: Room? = null,
) {
    var name by remember { mutableStateOf("") }
    var isPrivate by remember { mutableStateOf(false) }
    var hostIndex by remember { mutableStateOf(0) }
    var hostMenuOpen by remember { mutableStateOf(false) }
    var parent by remember { mutableStateOf<Room?>(null) }
    var parentMenuOpen by remember { mutableStateOf(false) }

    val host = supernodes.getOrNull(hostIndex)

    // Only rooms on the chosen host can be a parent: the Space tree belongs to
    // one supernode, so nesting across hosts is not a thing to offer.
    val candidates = remember(rooms, host) {
        rooms.filter { host != null && it.supernodeId == host.peerId }
    }

    // A sub-room lives on its parent's host. The room names it by whichever id
    // it was filed under, so match the supernode on either spelling.
    val fixedHostId = fixedParent?.let { p ->
        val bare = p.supernodeId.trimEnd('=')
        supernodes.firstOrNull { it.peerId == p.supernodeId || it.identityPub.trimEnd('=') == bare }
            ?.peerId ?: p.supernodeId
    }

    AlertDialog(
        onDismissRequest = onDismiss,
        title = {
            Text(
                if (fixedParent != null) {
                    "New room in ${fixedParent.roomName.ifBlank { fixedParent.roomId.take(12) }}"
                } else {
                    "New room"
                },
            )
        },
        text = {
            Column {
                OutlinedTextField(
                    value = name,
                    onValueChange = { name = it },
                    label = { Text("Room name") },
                    singleLine = true,
                    modifier = Modifier.fillMaxWidth(),
                )

                Spacer(Modifier.height(12.dp))

                Row(
                    modifier = Modifier
                        .fillMaxWidth()
                        .clickable { isPrivate = !isPrivate },
                    verticalAlignment = Alignment.CenterVertically,
                ) {
                    Checkbox(checked = isPrivate, onCheckedChange = { isPrivate = it })
                    Column(Modifier.padding(start = 4.dp)) {
                        Text("Private", style = MaterialTheme.typography.bodyMedium)
                        Text(
                            if (isPrivate) {
                                "Only people you invite can join."
                            } else {
                                "Anyone on this supernode can find and join it."
                            },
                            style = MaterialTheme.typography.labelSmall,
                            color = MaterialTheme.colorScheme.onSurfaceVariant,
                        )
                    }
                }

                if (fixedParent == null && candidates.isNotEmpty()) {
                    Spacer(Modifier.height(12.dp))
                    Box {
                        TextButton(onClick = { parentMenuOpen = true }) {
                            Text("Inside: ${parent?.roomName ?: "nothing"}")
                        }
                        DropdownMenu(
                            expanded = parentMenuOpen,
                            onDismissRequest = { parentMenuOpen = false },
                        ) {
                            DropdownMenuItem(
                                text = { Text("Nothing - top level") },
                                onClick = {
                                    parent = null
                                    parentMenuOpen = false
                                },
                            )
                            candidates.forEach { room ->
                                DropdownMenuItem(
                                    text = { Text(room.roomName.ifBlank { room.roomId.take(12) }) },
                                    onClick = {
                                        parent = room
                                        parentMenuOpen = false
                                    },
                                )
                            }
                        }
                    }
                }

                if (fixedParent == null && supernodes.size > 1) {
                    Spacer(Modifier.height(12.dp))
                    Box {
                        TextButton(onClick = { hostMenuOpen = true }) {
                            Text("Host: ${host?.label ?: "choose"}")
                        }
                        DropdownMenu(
                            expanded = hostMenuOpen,
                            onDismissRequest = { hostMenuOpen = false },
                        ) {
                            supernodes.forEachIndexed { index, node ->
                                DropdownMenuItem(
                                    text = { Text(node.label) },
                                    onClick = {
                                        hostIndex = index
                                        hostMenuOpen = false
                                    },
                                )
                            }
                        }
                    }
                }
            }
        },
        confirmButton = {
            TextButton(
                onClick = {
                    if (fixedParent != null && fixedHostId != null) {
                        onCreate(fixedHostId, name, isPrivate, fixedParent.roomId)
                    } else {
                        host?.let { onCreate(it.peerId, name, isPrivate, parent?.roomId.orEmpty()) }
                    }
                },
                enabled = name.isNotBlank() && (fixedParent != null || host != null),
            ) { Text("Create") }
        },
        dismissButton = { TextButton(onClick = onDismiss) { Text("Cancel") } },
    )
}

/**
 * Confirm before forgetting a peer.
 *
 * Removal drops the record and ends any call with them. It is recoverable -
 * a fresh invite puts them back - but not from inside this screen, so it is
 * worth one tap of friction.
 */
@Composable
private fun RemovePeerDialog(peer: Peer, onDismiss: () -> Unit, onConfirm: () -> Unit) {
    AlertDialog(
        onDismissRequest = onDismiss,
        title = { Text("Remove ${peer.label}?") },
        text = {
            Text(
                "This forgets the peer on this device and ends any call with them. " +
                    "Your chat history stays. You will need a new invite to reach " +
                    "them again.",
            )
        },
        confirmButton = {
            TextButton(onClick = onConfirm) {
                Text("Remove", color = MaterialTheme.colorScheme.error)
            }
        },
        dismissButton = { TextButton(onClick = onDismiss) { Text("Cancel") } },
    )
}

@Composable
private fun EmptyPeers(onCreateInvite: () -> Unit, onAcceptInvite: () -> Unit) {
    Column(
        modifier = Modifier.fillMaxSize().padding(32.dp),
        verticalArrangement = Arrangement.Center,
        horizontalAlignment = Alignment.CenterHorizontally,
    ) {
        Text("No peers yet", style = MaterialTheme.typography.titleMedium)
        Spacer(Modifier.height(8.dp))
        Text(
            "Trust is established by exchanging an invite. Send one, or paste one you were given.",
            style = MaterialTheme.typography.bodyMedium,
            color = MaterialTheme.colorScheme.onSurfaceVariant,
        )
        Spacer(Modifier.height(24.dp))
        Button(onClick = onCreateInvite) { Text("Create an invite") }
        Spacer(Modifier.height(8.dp))
        TextButton(onClick = onAcceptInvite) { Text("Accept an invite") }
    }
}

// ── Chat ───────────────────────────────────────────────────────────────────

@OptIn(ExperimentalMaterial3Api::class)
@Composable
private fun ChatScreen(
    peer: Peer,
    selfChat: Boolean,
    messages: List<ChatMessage>,
    onBack: () -> Unit,
    onSend: (String) -> Unit,
    onCall: () -> Unit,
    onRetry: (ChatMessage) -> Unit,
    onDelete: (ChatMessage) -> Unit,
    onAcceptInvite: (String) -> Unit,
    onJoinRoom: (String) -> Unit,
    joinableRoomIds: Set<String>,
    transfers: Map<String, Float>,
    fileRetries: Map<String, String>,
    onRetryFile: (String) -> Unit,
    onSendFile: (android.net.Uri) -> Unit,
    timeFormat: String,
) {
    var draft by remember { mutableStateOf("") }
    val listState = rememberLazyListState()
    val pinned = rememberPinnedToLatest(
        listState = listState,
        conversationKey = peer.peerId,
        itemCount = messages.size,
        latestKey = messages.lastOrNull()?.id,
    )

    // OpenDocument rather than GetContent: it returns a durable uri we can
    // read from for the length of a copy, which GetContent does not promise.
    val pickFile = rememberLauncherForActivityResult(
        ActivityResultContracts.OpenDocument(),
    ) { uri ->
        uri?.let {
            onSendFile(it)
            pinned.jumpToLatest()
        }
    }

    // Ask at the point of use rather than on launch: a client that demands
    // the microphone before you have placed a call is asking for something it
    // cannot yet justify. The in-app copy runs first because a call keeps
    // the mic on with the screen off.
    val requestMic = rememberExplainedPermission(
        permission = Manifest.permission.RECORD_AUDIO,
        title = MicRationaleTitle,
        body = MicRationaleBody,
        onGranted = onCall,
    )

    Column(Modifier.fillMaxSize().imePadding()) {
        TopAppBar(
            // The Scaffold above already pays the status-bar inset for this
            // content, and an M3 top bar applies its own by default - which
            // insets the bar twice and leaves a status-bar-height band of dead
            // space above the title. These bars live inside the Scaffold body
            // rather than its topBar slot, so the inset is not theirs to add.
            windowInsets = WindowInsets(0, 0, 0, 0),
            title = { Text(peer.label) },
            navigationIcon = {
                IconButton(onClick = onBack) {
                    Icon(Icons.AutoMirrored.Filled.ArrowBack, contentDescription = "Back")
                }
            },
            actions = {
                if (!selfChat) IconButton(onClick = { requestMic() }) {
                    Icon(Icons.Filled.Phone, contentDescription = "Call")
                }
            },
        )

        Box(Modifier.weight(1f).fillMaxWidth()) {
            LazyColumn(
                state = listState,
                modifier = Modifier.fillMaxSize().padding(horizontal = 12.dp),
                verticalArrangement = Arrangement.spacedBy(6.dp),
            ) {
                items(messages, key = { it.id }) { message ->
                    MessageBubble(
                        message = message,
                        onRetry = { onRetry(message) },
                        onDelete = { onDelete(message) },
                        onAcceptInvite = onAcceptInvite,
                        onJoinRoom = onJoinRoom,
                        joinableRoomIds = joinableRoomIds,
                        fileRetries = fileRetries,
                        onRetryFile = onRetryFile,
                        timeFormat = timeFormat,
                    )
                }
            }

            JumpToCurrentButton(visible = pinned.scrolledAway.value, onClick = pinned.jumpToLatest)
        }

        // One bar per transfer in flight, sending or receiving. A file moving
        // is the only thing here slow enough to need one, so it takes space
        // only while it is happening.
        transfers.forEach { (_, progress) ->
            LinearProgressIndicator(
                progress = { progress },
                modifier = Modifier.fillMaxWidth().padding(horizontal = 12.dp),
            )
        }

        Row(
            modifier = Modifier.fillMaxWidth().padding(12.dp),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            if (!selfChat) IconButton(onClick = { pickFile.launch(arrayOf("*/*")) }) {
                Icon(Icons.Filled.AddCircle, contentDescription = "Attach a file")
            }
            OutlinedTextField(
                value = draft,
                onValueChange = { draft = it },
                placeholder = { Text("Message") },
                modifier = Modifier.weight(1f),
                maxLines = 4,
            )
            Spacer(Modifier.width(8.dp))
            IconButton(
                onClick = {
                    onSend(draft)
                    draft = ""
                    pinned.jumpToLatest()
                },
                enabled = draft.isNotBlank(),
            ) {
                Icon(Icons.AutoMirrored.Filled.Send, contentDescription = "Send")
            }
        }
    }
}

@OptIn(ExperimentalFoundationApi::class)
@Composable
private fun MessageBubble(
    message: ChatMessage,
    onRetry: () -> Unit,
    onDelete: () -> Unit,
    onAcceptInvite: (String) -> Unit,
    onJoinRoom: (String) -> Unit,
    joinableRoomIds: Set<String>,
    fileRetries: Map<String, String> = emptyMap(),
    onRetryFile: (String) -> Unit = {},
    timeFormat: String = DateTimeFormats.DEFAULT,
) {
    val invite = remember(message.body) { findInviteUrl(message.body) }
    // With the link lifted into the card, a message that was only a link has
    // no text left worth a bubble.
    val text = if (invite == null) message.body else bodyWithoutInvite(message.body, invite)
    val hasAttachment = message.attachmentName.isNotBlank() || message.attachmentPath.isNotBlank()
    var menuOpen by remember { mutableStateOf(false) }
    val clipboard = androidx.compose.ui.platform.LocalClipboardManager.current
    val alignment = if (message.isSelf) Alignment.End else Alignment.Start
    val container = if (message.isSelf) {
        MaterialTheme.colorScheme.primaryContainer
    } else {
        MaterialTheme.colorScheme.surfaceVariant
    }

    Column(Modifier.fillMaxWidth(), horizontalAlignment = alignment) {
        Box {
            Card(
                colors = CardDefaults.cardColors(containerColor = container),
                modifier = Modifier.combinedClickable(
                    onClick = {},
                    onLongClick = { menuOpen = true },
                ),
            ) {
                // The body of an attachment message is only its label, so the
                // file itself takes that place — a picture shows as a picture.
                if (hasAttachment) {
                    val transferId = transferIdFromMessage(message.id)
                    val retryReason = transferId?.let { fileRetries[it] }
                    AttachmentContent(
                        kind = message.kind,
                        name = message.attachmentName,
                        path = message.attachmentPath,
                        sizeStr = message.sizeStr,
                        modifier = Modifier.padding(10.dp),
                        retryReason = retryReason,
                        onRetry = transferId?.takeIf { retryReason != null }?.let { id ->
                            { onRetryFile(id) }
                        },
                    )
                } else {
                    Text(text, modifier = Modifier.padding(10.dp))
                }
            }

            DropdownMenu(expanded = menuOpen, onDismissRequest = { menuOpen = false }) {
                DropdownMenuItem(
                    text = { Text("Copy") },
                    onClick = {
                        clipboard.setText(
                            androidx.compose.ui.text.AnnotatedString(message.body),
                        )
                        menuOpen = false
                    },
                )
                // Retry only where it can do something: a failed message of
                // our own. Offering it on a delivered one invites a duplicate.
                if (message.isSelf && message.status == "failed") {
                    DropdownMenuItem(
                        text = { Text("Try again") },
                        onClick = {
                            onRetry()
                            menuOpen = false
                        },
                    )
                }
                DropdownMenuItem(
                    text = { Text("Delete", color = MaterialTheme.colorScheme.error) },
                    onClick = {
                        onDelete()
                        menuOpen = false
                    },
                )
            }
        }
        invite?.let {
            Spacer(Modifier.height(4.dp))
            InviteEmbed(
                url = it,
                mine = message.isSelf,
                onAccept = onAcceptInvite,
                onJoinRoom = onJoinRoom,
                joinableRoomIds = joinableRoomIds,
            )
        }
        // A failed send is the one status worth spending a line on — the rest
        // (sending, sent, delivered) resolve on their own within a second.
        val note = if (message.status == "failed") {
            message.statusNote.ifBlank { "not delivered" }
        } else {
            DateTimeFormats.format(message.timestamp, timeFormat)
        }
        Text(
            note,
            style = MaterialTheme.typography.labelSmall,
            color = if (message.status == "failed") {
                MaterialTheme.colorScheme.error
            } else {
                MaterialTheme.colorScheme.onSurfaceVariant
            },
            modifier = Modifier.padding(horizontal = 4.dp),
        )
    }
}

@OptIn(ExperimentalMaterial3Api::class)
@Composable
private fun RoomChatScreen(
    room: Room,
    messages: List<RoomMessage>,
    avatars: Map<String, AvatarArt>,
    peers: List<Peer>,
    /** Voice participants - drives the "in voice" bar only. */
    members: List<String>,
    /** Everyone in the room, voice or text - drives the header count. */
    chatMembers: List<String>,
    joined: Boolean,
    voiceActive: Boolean,
    muted: Boolean,
    videoActive: Boolean,
    speakerphone: Boolean,
    headsetAttached: Boolean,
    onBack: () -> Unit,
    onSend: (String) -> Unit,
    onJoinVoice: () -> Unit,
    onLeaveVoice: () -> Unit,
    onToggleMute: () -> Unit,
    onToggleSpeaker: () -> Unit,
    onToggleVideo: (Boolean) -> Unit,
    onAcceptInvite: (String) -> Unit,
    onJoinRoom: (String) -> Unit,
    transfers: Map<String, Float>,
    fileRetries: Map<String, String>,
    onRetryFile: (String) -> Unit,
    onSendFile: (android.net.Uri) -> Unit,
    onShare: () -> Unit,
    state: AppState,
    /** This room's Voice and Text-only leaves, for the members sheet. */
    sheetRows: List<TreeRow>,
    treeFold: TreeFold,
    treeActions: RoomTreeActions,
) {
    var draft by remember { mutableStateOf("") }
    var membersOpen by remember { mutableStateOf(false) }
    val listState = rememberLazyListState()
    val pinned = rememberPinnedToLatest(
        listState = listState,
        conversationKey = room.key,
        itemCount = messages.size,
        latestKey = messages.lastOrNull()?.messageId,
    )

    val pickFile = rememberLauncherForActivityResult(
        ActivityResultContracts.OpenDocument(),
    ) { uri ->
        uri?.let {
            onSendFile(it)
            pinned.jumpToLatest()
        }
    }
    val context = LocalContext.current

    val requestMic = rememberExplainedPermission(
        permission = Manifest.permission.RECORD_AUDIO,
        title = MicRationaleTitle,
        body = MicRationaleBody,
        onGranted = onJoinVoice,
    )

    // CameraX must be bound before the core asks for video — the native side
    // waits for a first frame to learn the capture size.
    val requestCamera = rememberExplainedPermission(
        permission = Manifest.permission.CAMERA,
        title = CameraRationaleTitle,
        body = CameraRationaleBody,
        onGranted = {
            CameraCapture.start(context)
            onToggleVideo(true)
        },
    )

    Column(Modifier.fillMaxSize().imePadding()) {
        val voiceHere = state.voiceRoom?.roomId == room.roomId
        val parentName = state.rooms
            .firstOrNull { room.parentId.isNotBlank() && it.roomId == room.parentId }
            ?.roomName?.ifBlank { null }
        val joinableRoomIds = remember(state.rooms) { state.rooms.map { it.roomId }.toSet() }
        TopAppBar(
            // The Scaffold above already pays the status-bar inset for this
            // content, and an M3 top bar applies its own by default - which
            // insets the bar twice and leaves a status-bar-height band of dead
            // space above the title. These bars live inside the Scaffold body
            // rather than its topBar slot, so the inset is not theirs to add.
            windowInsets = WindowInsets(0, 0, 0, 0),
            title = {
                Column {
                    Text(
                        room.roomName.ifBlank { room.roomId.take(12) },
                        maxLines = 1,
                        overflow = TextOverflow.Ellipsis,
                    )
                    Text(
                        listOfNotNull(
                            parentName,
                            if (joined) "${chatMembers.size} here" else "joining...",
                        ).joinToString(" · "),
                        style = MaterialTheme.typography.labelSmall,
                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                        maxLines = 1,
                        overflow = TextOverflow.Ellipsis,
                    )
                }
            },
            navigationIcon = {
                IconButton(onClick = onBack) {
                    Icon(Icons.AutoMirrored.Filled.ArrowBack, contentDescription = "Leave room")
                }
            },
            actions = {
                // Voice is joined here and left from the dock at the foot of
                // the screen, where the call lives on every screen.
                if (joined && state.voiceRoom == null) {
                    TextButton(onClick = requestMic) { Text("Join voice") }
                }
                // Who is here, in voice or reading, and what you can do about
                // each of them: the same rows as the Rooms tree.
                IconButton(enabled = joined, onClick = { membersOpen = true }) {
                    Icon(painterResource(R.drawable.ds_peers), contentDescription = "Members")
                }
                IconButton(onClick = onShare) {
                    Icon(Icons.Filled.Share, contentDescription = "Share this room")
                }
            },
        )

        // Watched video, full width at the top of the room it comes from.
        // Elsewhere it floats (FloatingVideo), so it never quietly goes away.
        if (voiceHere) {
            val myKey = state.identity.publicId.videoKey()
            val shown = state.roomVoiceRosters.roomMembersUnion(room.roomId)
                .filter { it.videoKey() != myKey && state.watching(it) && state.cameraOn(it) }
            var fullScreen by remember { mutableStateOf<String?>(null) }
            if (shown.size == 1) {
                val id = shown[0]
                VideoTile(
                    peerId = id,
                    name = peers.roomSenderName(id, ""),
                    size = state.videoSizes[id.videoKey()],
                    stalled = id.videoKey() in state.stalledVideo,
                    modifier = Modifier.fillMaxWidth().heightIn(max = 320.dp),
                    onClick = { fullScreen = id },
                )
            } else if (shown.size > 1) {
                Row(
                    modifier = Modifier
                        .fillMaxWidth()
                        .background(Color.Black)
                        .horizontalScroll(rememberScrollState()),
                ) {
                    shown.forEach { id ->
                        VideoTile(
                            peerId = id,
                            name = peers.roomSenderName(id, ""),
                            size = state.videoSizes[id.videoKey()],
                            stalled = id.videoKey() in state.stalledVideo,
                            modifier = Modifier.height(200.dp),
                            onClick = { fullScreen = id },
                        )
                    }
                }
            }
            // Closes itself when its peer stops being shown, rather than
            // holding a black screen open.
            fullScreen?.takeIf { it in shown }?.let { id ->
                FullScreenVideo(
                    peerId = id,
                    name = peers.roomSenderName(id, ""),
                    size = state.videoSizes[id.videoKey()],
                    stalled = id.videoKey() in state.stalledVideo,
                    onDismiss = { fullScreen = null },
                )
            }
        }

        if (messages.isEmpty()) {
            Column(
                modifier = Modifier.weight(1f).fillMaxWidth().padding(32.dp),
                verticalArrangement = Arrangement.Center,
                horizontalAlignment = Alignment.CenterHorizontally,
            ) {
                Text(
                    // Worth stating plainly rather than showing a blank pane:
                    // rooms are ephemeral on the supernode and room chat is
                    // never written to the local store, so there is no history
                    // to load - only what arrives from now on.
                    if (joined) {
                        "Messages appear from now on. Room chat is not stored on this device."
                    } else {
                        "Waiting for the room to admit you."
                    },
                    style = MaterialTheme.typography.bodyMedium,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                    textAlign = TextAlign.Center,
                )
            }
        } else {
            Box(Modifier.weight(1f).fillMaxWidth()) {
                LazyColumn(
                    state = listState,
                    modifier = Modifier.fillMaxSize().padding(horizontal = 12.dp),
                    verticalArrangement = Arrangement.spacedBy(6.dp),
                ) {
                    items(messages, key = { it.messageId }) {
                        RoomMessageBubble(
                            message = it,
                            avatar = avatars[it.senderId],
                            senderName = peers.roomSenderName(it.senderId, it.senderHandle),
                            onAcceptInvite = onAcceptInvite,
                            onJoinRoom = onJoinRoom,
                            joinableRoomIds = joinableRoomIds,
                            fileRetries = fileRetries,
                            onRetryFile = onRetryFile,
                            timeFormat = state.prefs.timeFormat,
                        )
                    }
                }

                JumpToCurrentButton(
                    visible = pinned.scrolledAway.value,
                    onClick = pinned.jumpToLatest,
                )
            }
        }

        transfers.forEach { (_, progress) ->
            LinearProgressIndicator(
                progress = { progress },
                modifier = Modifier.fillMaxWidth().padding(horizontal = 12.dp),
            )
        }

        Row(
            modifier = Modifier.fillMaxWidth().padding(12.dp),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            IconButton(
                onClick = { pickFile.launch(arrayOf("*/*")) },
                enabled = joined,
            ) {
                Icon(Icons.Filled.AddCircle, contentDescription = "Share a file")
            }
            OutlinedTextField(
                value = draft,
                onValueChange = { draft = it },
                placeholder = { Text(if (joined) "Message" else "Joining...") },
                enabled = joined,
                modifier = Modifier.weight(1f),
                maxLines = 4,
            )
            Spacer(Modifier.width(8.dp))
            IconButton(
                onClick = {
                    onSend(draft)
                    draft = ""
                    pinned.jumpToLatest()
                },
                enabled = joined && draft.isNotBlank(),
            ) {
                Icon(Icons.AutoMirrored.Filled.Send, contentDescription = "Send")
            }
        }
    }

    if (membersOpen) {
        // The room's Voice and Text-only leaves, drawn by the same rows as the
        // Rooms tree. Watching and messaging close the sheet, since what they
        // open is behind it; inviting stays.
        val sheetActions = RoomTreeActions(
            onFold = treeActions.onFold,
            onOpenRoom = treeActions.onOpenRoom,
            onJoinVoice = { r ->
                membersOpen = false
                treeActions.onJoinVoice(r)
            },
            onSetHidden = treeActions.onSetHidden,
            onCreateSubRoom = treeActions.onCreateSubRoom,
            onTogglePin = treeActions.onTogglePin,
            onMoveRoom = treeActions.onMoveRoom,
            onCopyInvite = treeActions.onCopyInvite,
            onInviteContact = treeActions.onInviteContact,
            onSetMessageAlerts = treeActions.onSetMessageAlerts,
            members = MemberActions(
                onToggleWatch = { id ->
                    membersOpen = false
                    treeActions.members.onToggleWatch(id)
                },
                onMessage = { peer ->
                    membersOpen = false
                    treeActions.members.onMessage(peer)
                },
                onInvite = treeActions.members.onInvite,
            ),
            onPeerAudio = treeActions.onPeerAudio,
        )
        ModalBottomSheet(onDismissRequest = { membersOpen = false }) {
            LazyColumn(Modifier.fillMaxWidth().padding(bottom = 24.dp)) {
                item {
                    Text(
                        "${room.roomName.ifBlank { "Room" }} · ${chatMembers.size}",
                        style = MaterialTheme.typography.titleMedium,
                        modifier = Modifier.padding(horizontal = 16.dp, vertical = 8.dp),
                    )
                }
                roomTreeItems(sheetRows, state, treeFold, sheetActions)
            }
        }
    }
}

@Composable
private fun RoomMessageBubble(
    message: RoomMessage,
    avatar: AvatarArt?,
    senderName: String,
    onAcceptInvite: (String) -> Unit,
    onJoinRoom: (String) -> Unit,
    joinableRoomIds: Set<String>,
    fileRetries: Map<String, String> = emptyMap(),
    onRetryFile: (String) -> Unit = {},
    timeFormat: String = DateTimeFormats.DEFAULT,
) {
    val invite = remember(message.body) { findInviteUrl(message.body) }
    val text = if (invite == null) message.body else bodyWithoutInvite(message.body, invite)
    val alignment = if (message.isSelf) Alignment.End else Alignment.Start
    val container = if (message.isSelf) {
        MaterialTheme.colorScheme.primaryContainer
    } else {
        MaterialTheme.colorScheme.surfaceVariant
    }

    // Rooms only, matching the desktop: in a 1:1 chat the same two faces beside
    // every line are noise, but in a room the face is how you tell speakers
    // apart at a glance, ahead of reading the name.
    Row(
        modifier = Modifier.fillMaxWidth(),
        horizontalArrangement = if (message.isSelf) Arrangement.End else Arrangement.Start,
        verticalAlignment = Alignment.Top,
    ) {
        if (!message.isSelf) {
            RoomAvatarSlot(avatar)
            Spacer(Modifier.width(8.dp))
        }
        Column(horizontalAlignment = alignment) {
            // Unlike a 1:1 chat, a room has many senders, so each message has to
            // say who wrote it.
            if (!message.isSelf) {
                Text(
                    senderName,
                    style = MaterialTheme.typography.labelSmall,
                    color = MaterialTheme.colorScheme.primary,
                    modifier = Modifier.padding(horizontal = 4.dp),
                )
            }
            val hasAttachment =
                message.attachmentName.isNotBlank() || message.attachmentPath.isNotBlank()
            if (hasAttachment) {
                Card(colors = CardDefaults.cardColors(containerColor = container)) {
                    val transferId = transferIdFromMessage(message.messageId)
                    val retryReason = transferId?.let { fileRetries[it] }
                    AttachmentContent(
                        kind = message.kind,
                        name = message.attachmentName,
                        path = message.attachmentPath,
                        sizeStr = message.sizeStr,
                        modifier = Modifier.padding(10.dp),
                        retryReason = retryReason,
                        onRetry = transferId?.takeIf { retryReason != null }?.let { id ->
                            { onRetryFile(id) }
                        },
                    )
                }
            } else if (text.isNotBlank()) {
                Card(colors = CardDefaults.cardColors(containerColor = container)) {
                    Text(text, modifier = Modifier.padding(10.dp))
                }
            }
            invite?.let {
                Spacer(Modifier.height(4.dp))
                InviteEmbed(
                    url = it,
                    mine = message.isSelf,
                    onAccept = onAcceptInvite,
                    onJoinRoom = onJoinRoom,
                    joinableRoomIds = joinableRoomIds,
                )
            }
            Text(
                DateTimeFormats.format(message.timestamp, timeFormat),
                style = MaterialTheme.typography.labelSmall,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
                modifier = Modifier.padding(horizontal = 4.dp),
            )
        }
        if (message.isSelf) {
            Spacer(Modifier.width(8.dp))
            RoomAvatarSlot(avatar)
        }
    }
}

/**
 * A room member asks to become trusted peers.
 *
 * Only the oldest offer is shown; the rest wait behind it, so a second offer
 * cannot swap the face under a tap aimed at the first. The invite inside lives
 * 15 minutes, so an offer left that long goes quietly rather than failing when
 * accepted.
 */
@Composable
private fun TrustOfferDialog(
    offer: TrustOffer,
    roomName: String,
    avatar: AvatarArt?,
    waiting: Int,
    onAnswer: (Boolean) -> Unit,
) {
    LaunchedEffect(offer) {
        val left = TRUST_OFFER_LIFETIME_MS - (System.currentTimeMillis() - offer.receivedAtMs)
        kotlinx.coroutines.delay(left.coerceAtLeast(0))
        onAnswer(false)
    }
    AlertDialog(
        onDismissRequest = { onAnswer(false) },
        icon = { avatar?.let { Avatar(it, Modifier.size(48.dp)) } },
        title = { Text("Become trusted peers?") },
        text = {
            Column {
                // The handle is the name they chose; the id beneath it is
                // what actually identifies them.
                Text(
                    "${offer.handle.ifBlank { "A room member" }} in $roomName " +
                        "asked to add you as a trusted peer.",
                )
                Spacer(Modifier.height(4.dp))
                Text(
                    offer.senderId,
                    style = MaterialTheme.typography.labelSmall,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                    maxLines = 1,
                    overflow = TextOverflow.Ellipsis,
                )
                Spacer(Modifier.height(8.dp))
                Text(
                    "Trusted peers can message you, call you and send you files outside the room.",
                    style = MaterialTheme.typography.bodySmall,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
                if (waiting > 0) {
                    Spacer(Modifier.height(4.dp))
                    Text("$waiting more waiting", style = MaterialTheme.typography.labelSmall)
                }
            }
        },
        confirmButton = { TextButton(onClick = { onAnswer(true) }) { Text("Accept") } },
        dismissButton = { TextButton(onClick = { onAnswer(false) }) { Text("Not now") } },
    )
}

/** A minute short of the invite's own 15, so an accept never races its expiry. */
private const val TRUST_OFFER_LIFETIME_MS = 14 * 60 * 1000L

/**
 * One avatar beside a room message, holding its space while the art loads.
 *
 * Avatars arrive asynchronously - the core renders the SVG on request - so an
 * absent one reserves the same box rather than collapsing, which would shuffle
 * every bubble sideways the moment it appeared.
 */
@Composable
private fun RoomAvatarSlot(avatar: AvatarArt?) {
    if (avatar != null) {
        Avatar(avatar, Modifier.size(ROOM_AVATAR_SIZE))
    } else {
        Spacer(Modifier.size(ROOM_AVATAR_SIZE))
    }
}

/** Matches the desktop's 32px room avatar. */
private val ROOM_AVATAR_SIZE = 32.dp

/**
 * Confirm before locking.
 *
 * Locking ends the session, drops the connection to every peer, and - when a
 * key is stored - forgets it, so the way back in is the passphrase. That is
 * too much to hang off one stray tap in a top bar.
 */
@Composable
private fun LockIdentityDialog(
    stayUnlocked: Boolean,
    onDismiss: () -> Unit,
    onConfirm: () -> Unit,
) {
    AlertDialog(
        onDismissRequest = onDismiss,
        title = { Text("Lock identity?") },
        text = {
            Text(
                if (stayUnlocked) {
                    "This signs out, disconnects your peers, and forgets the key kept " +
                        "on this device. You will need your passphrase to open DoubleSlash again."
                } else {
                    "This signs out and disconnects your peers. You will need your " +
                        "passphrase to open DoubleSlash again."
                },
            )
        },
        confirmButton = {
            TextButton(onClick = onConfirm) {
                Text("Lock", color = MaterialTheme.colorScheme.error)
            }
        },
        dismissButton = { TextButton(onClick = onDismiss) { Text("Cancel") } },
    )
}

// ── Portal ───────────────────────────────────────────────────────

/**
 * A supernode's in-app portal.
 *
 * There is no network protocol behind a portal - the supernode serves the
 * pages over the identity QUIC relay - so every request is intercepted and
 * answered by the core. The pages are served from
 * [PortalBridge.PORTAL_ORIGIN] rather than `d://` because WebView cannot
 * register a scheme's capabilities the way the desktop does, and Chromium
 * refuses an unregistered scheme for module scripts; nothing reaches the
 * network either way.
 *
 * JavaScript is on because that is the entire point of the portal; what makes
 * it defensible is that the pages come from a supernode the user has already
 * trusted enough to relay their traffic, over an authenticated channel, and
 * the WebView is given no file or content access.
 */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
private fun PortalScreen(
    supernodeId: String,
    label: String,
    myPeerId: String,
    core: com.doubleslash.client.DoubleSlashCore,
    onBack: () -> Unit,
) {
    val bridge = remember(supernodeId, myPeerId) {
        com.doubleslash.client.PortalBridge(core, supernodeId, myPeerId)
    }

    Column(Modifier.fillMaxSize()) {
        TopAppBar(
            windowInsets = WindowInsets(0, 0, 0, 0),
            title = { Text(label) },
            navigationIcon = {
                IconButton(onClick = onBack) {
                    Icon(Icons.AutoMirrored.Filled.ArrowBack, contentDescription = "Leave portal")
                }
            },
        )

        AndroidView(
            // `weight`, not `fillMaxSize`: a Column measures a non-weighted
            // child with an unbounded main axis, and the WebView below needs a
            // bounded one to be given a definite height. See the layoutParams
            // note in the factory for why a definite height matters so much.
            modifier = Modifier.weight(1f).fillMaxWidth(),
            factory = { context ->
                // Debuggable builds only, and it changes nothing the page can
                // see: it exposes this WebView to Chrome DevTools over adb, so
                // a portal page that misbehaves can be inspected directly
                // instead of being guessed at from the outside. A release APK
                // is not debuggable, so this stays off in shipped builds.
                val debuggable = context.applicationInfo.flags and
                    android.content.pm.ApplicationInfo.FLAG_DEBUGGABLE != 0
                if (debuggable) {
                    android.webkit.WebView.setWebContentsDebuggingEnabled(true)
                }

                android.webkit.WebView(context).apply {
                    // Without this the WebView carries no LayoutParams, so
                    // AndroidView treats it as WRAP_CONTENT and measures it
                    // AT_MOST. Chromium reads that as "size to content" and
                    // lays the document out against a zero-height viewport:
                    // `html` comes out 0px tall while `clientHeight` still
                    // reports the real 770, and every height that resolves
                    // against the viewport - `vh`, `dvh`, `svh`, `lvh` and
                    // percentages alike - computes to zero. Any portal page
                    // that sizes itself off the viewport then collapses. The
                    // demo shell's `.app { height: 100dvh }` became 0px and
                    // the game canvas under it laid out 2px tall while
                    // happily drawing into a 1600x1200 buffer, so the games
                    // ran perfectly and were invisible.
                    //
                    // MATCH_PARENT plus the bounded constraint from `weight`
                    // above makes AndroidView emit an EXACTLY spec instead.
                    layoutParams = android.view.ViewGroup.LayoutParams(
                        android.view.ViewGroup.LayoutParams.MATCH_PARENT,
                        android.view.ViewGroup.LayoutParams.MATCH_PARENT,
                    )
                    settings.javaScriptEnabled = true
                    settings.domStorageEnabled = true
                    // No local file or content-provider reach: a portal page is
                    // remote code, and the only thing it may touch is the
                    // bridge below.
                    settings.allowFileAccess = false
                    settings.allowContentAccess = false
                    settings.mixedContentMode =
                        android.webkit.WebSettings.MIXED_CONTENT_NEVER_ALLOW

                    addJavascriptInterface(bridge.PortalApi(), "__doubleslashNative")

                    // A portal page had no way to report anything: with no
                    // WebChromeClient, WebView drops every console message on
                    // the floor, so a page that threw on load looked exactly
                    // like a page that did nothing. These go to logcat under
                    // the PortalPage tag.
                    webChromeClient = object : android.webkit.WebChromeClient() {
                        override fun onConsoleMessage(
                            message: android.webkit.ConsoleMessage,
                        ): Boolean {
                            android.util.Log.i(
                                "PortalPage",
                                "${message.message()} " +
                                    "(${message.sourceId()}:${message.lineNumber()})",
                            )
                            return true
                        }
                    }

                    webViewClient = object : android.webkit.WebViewClient() {
                        override fun shouldInterceptRequest(
                            view: android.webkit.WebView,
                            request: android.webkit.WebResourceRequest,
                        ): android.webkit.WebResourceResponse? {
                            val intercepted = bridge.interceptRequest(request)
                            if (intercepted != null) return intercepted
                            // Anything the portal does not answer would
                            // otherwise hit the network with the JS bridge
                            // still attached.
                            return android.webkit.WebResourceResponse(
                                "text/plain",
                                "utf-8",
                                403,
                                "Blocked",
                                emptyMap(),
                                ByteArray(0).inputStream(),
                            )
                        }

                        override fun onReceivedError(
                            view: android.webkit.WebView,
                            request: android.webkit.WebResourceRequest,
                            error: android.webkit.WebResourceError,
                        ) {
                            android.util.Log.w(
                                "PortalPage",
                                "load failed ${request.url}: ${error.description}",
                            )
                        }

                        override fun shouldOverrideUrlLoading(
                            view: android.webkit.WebView,
                            request: android.webkit.WebResourceRequest,
                        ): Boolean {
                            val url = request.url
                            val scheme = url.scheme.orEmpty()
                            // The portal's own origin and the hand-off
                            // spellings stay in the WebView; everything else
                            // is someone else's site and goes to the browser.
                            if (scheme.equals("d", true) ||
                                scheme.equals("doubleslash", true) ||
                                url.host == com.doubleslash.client.PortalBridge.PORTAL_HOST
                            ) {
                                return false
                            }
                            Legal.openUrl(view.context, url.toString())
                            return true
                        }
                    }

                    // Not `d://$supernodeId/...`: see PortalBridge.PORTAL_HOST.
                    // WebView cannot register a custom scheme, so on `d://`
                    // Chromium refuses every module script in the portal.
                    loadUrl("${com.doubleslash.client.PortalBridge.PORTAL_ORIGIN}/index.html")
                }
            },
        )
    }
}

// ── Settings ──────────────────────────────────────────────────────────────

/**
 * The handful of settings that mean something on a phone.
 *
 * The desktop persists about sixty; most of the rest are device pickers,
 * window geometry and tray behaviour that a phone either decides for itself
 * or does not have. Each control here changes what the next call or launch
 * actually does.
 */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
private fun SettingsScreen(
    state: AppState,
    onBack: () -> Unit,
    onSetHandle: (String) -> Unit,
    onSetFrontCamera: (Boolean) -> Unit,
    onSetVoiceActivation: (Boolean) -> Unit,
    onSetTheme: (String) -> Unit,
    onSetFontScale: (Int) -> Unit,
    onSetTimeFormat: (String) -> Unit,
    onSetAvatarConfig: (String) -> Unit,
    onPreviewAvatar: suspend (String) -> AvatarArt?,
    onPurgeHistory: () -> Unit,
    onTrimHistory: (Int) -> Unit,
    onSetInputGain: (Int) -> Unit,
    onSetOutputGain: (Int) -> Unit,
    onSetNoiseStrength: (Int) -> Unit,
    onSetVoiceBitrate: (Int) -> Unit,
    onRemoveSupernode: (String) -> Unit,
    onOpenPortal: (SupernodeInfo) -> Unit,
    onSetSkin: (String) -> Unit,
    onSetStayUnlocked: (Boolean) -> Unit,
    onLock: () -> Unit,
    version: String,
) {
    var handle by remember(state.identity.handle) { mutableStateOf(state.identity.handle) }
    var showAvatarEditor by remember { mutableStateOf(false) }
    var confirmPurge by remember { mutableStateOf(false) }
    var showLicenses by remember { mutableStateOf(false) }
    var confirmRemoveNode by remember { mutableStateOf<SupernodeInfo?>(null) }

    var confirmLock by remember { mutableStateOf(false) }
    val clipboard = androidx.compose.ui.platform.LocalClipboardManager.current
    val scroll = rememberScrollState()
    val scope = rememberCoroutineScope()
    // Where each section starts in the scrolling column, for the chips.
    val anchors = remember { mutableStateMapOf<String, Int>() }
    val sections = listOf("Identity", "Voice", "Text", "Date", "Appearance", "Network", "Privacy", "About")

    @Composable
    fun SectionTitle(name: String) {
        Text(
            name,
            style = MaterialTheme.typography.titleMedium,
            modifier = Modifier
                .padding(top = 8.dp, bottom = 8.dp)
                .onGloballyPositioned { anchors[name] = it.positionInParent().y.toInt() },
        )
    }

    Column(Modifier.fillMaxSize()) {
        TopAppBar(
            windowInsets = WindowInsets(0, 0, 0, 0),
            title = { Text("Settings") },
            navigationIcon = {
                IconButton(onClick = onBack) {
                    Icon(Icons.AutoMirrored.Filled.ArrowBack, contentDescription = "Back")
                }
            },
        )

        // The desktop's settings sections, a tap away. The avatar opens this
        // screen at Identity; every other section is one chip along.
        Row(
            horizontalArrangement = Arrangement.spacedBy(8.dp),
            modifier = Modifier
                .fillMaxWidth()
                .horizontalScroll(rememberScrollState())
                .padding(horizontal = 12.dp, vertical = 6.dp),
        ) {
            sections.forEach { name ->
                FilterChip(
                    selected = false,
                    onClick = { scope.launch { scroll.animateScrollTo(anchors[name] ?: 0) } },
                    label = { Text(name) },
                )
            }
        }
        HorizontalDivider()

        Column(
            Modifier
                .fillMaxSize()
                .verticalScroll(scroll)
                .padding(16.dp),
        ) {
            // ── Identity ──────────────────────────────────────────────────
            SectionTitle("Identity")
            Row(verticalAlignment = Alignment.CenterVertically) {
                state.avatars[state.identity.peerId]?.let { art ->
                    Avatar(art, Modifier.size(56.dp))
                    Spacer(Modifier.width(12.dp))
                }
                Column(Modifier.weight(1f)) {
                    Text(
                        state.identity.handle.ifBlank { "No name set" },
                        style = MaterialTheme.typography.titleSmall,
                    )
                    Text(
                        state.identity.publicId,
                        style = MaterialTheme.typography.labelSmall,
                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                        maxLines = 1,
                        overflow = TextOverflow.Ellipsis,
                    )
                }
                TextButton(
                    enabled = state.identity.publicId.isNotBlank(),
                    onClick = {
                        clipboard.setText(androidx.compose.ui.text.AnnotatedString(state.identity.publicId))
                    },
                ) { Text("Copy ID") }
            }

            Spacer(Modifier.height(16.dp))
            Text("Your name", style = MaterialTheme.typography.titleSmall)
            Spacer(Modifier.height(8.dp))
            OutlinedTextField(
                value = handle,
                onValueChange = { handle = it },
                placeholder = { Text("Not set") },
                supportingText = {
                    Text("What peers see instead of your ID. Changing it tells them.")
                },
                singleLine = true,
                modifier = Modifier.fillMaxWidth(),
            )
            Spacer(Modifier.height(8.dp))
            Button(
                onClick = { onSetHandle(handle) },
                enabled = handle.trim() != state.identity.handle,
            ) { Text("Save name") }

            Spacer(Modifier.height(16.dp))
            Text("Your avatar", style = MaterialTheme.typography.titleSmall)
            Spacer(Modifier.height(8.dp))
            Row(verticalAlignment = Alignment.CenterVertically) {
                Text(
                    "Drawn from your identity, so it is the same everywhere.",
                    style = MaterialTheme.typography.labelSmall,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                    modifier = Modifier.weight(1f),
                )
                TextButton(onClick = { showAvatarEditor = true }) { Text("Edit") }
            }

            BackupButton(unlocked = true)

            Spacer(Modifier.height(16.dp))
            Text("This device", style = MaterialTheme.typography.titleSmall)
            // The same choice as the unlock screen, reachable after the fact:
            // changing your mind should not require locking yourself out.
            SettingSwitch(
                title = "Stay unlocked",
                subtitle = if (state.stayUnlocked) {
                    "Opens without your passphrase"
                } else {
                    "Asks for your passphrase each launch"
                },
                checked = state.stayUnlocked,
                onCheckedChange = onSetStayUnlocked,
            )
            TextButton(onClick = { confirmLock = true }) {
                Text("Lock identity", color = MaterialTheme.colorScheme.error)
            }

            Spacer(Modifier.height(16.dp))
            HorizontalDivider()
            Spacer(Modifier.height(8.dp))

            // ── Voice ─────────────────────────────────────────────────────
            SectionTitle("Voice")
            SettingSwitch(
                title = "Front camera",
                subtitle = if (state.prefs.frontCamera) {
                    "Video calls open with the selfie camera"
                } else {
                    "Video calls open with the rear camera"
                },
                checked = state.prefs.frontCamera,
                onCheckedChange = onSetFrontCamera,
            )
            SettingSwitch(
                title = "Voice activation",
                subtitle = if (state.prefs.voiceActivation) {
                    "The mic opens when you speak"
                } else {
                    "The mic stays open for the whole call"
                },
                checked = state.prefs.voiceActivation,
                onCheckedChange = onSetVoiceActivation,
            )
            Spacer(Modifier.height(8.dp))
            TuningSlider(
                label = "Microphone",
                value = state.prefs.inputGain.toFloat(),
                range = 0f..200f,
                display = "${state.prefs.inputGain}%",
                onChange = { onSetInputGain(it.toInt()) },
            )
            TuningSlider(
                label = "Speaker",
                value = state.prefs.outputGain.toFloat(),
                range = 0f..200f,
                display = "${state.prefs.outputGain}%",
                onChange = { onSetOutputGain(it.toInt()) },
            )
            TuningSlider(
                label = "Noise gate",
                value = state.prefs.noiseStrength.toFloat(),
                range = 0f..4f,
                steps = 3,
                display = when (state.prefs.noiseStrength) {
                    0 -> "off"
                    1 -> "mild"
                    2 -> "moderate"
                    3 -> "aggressive"
                    else -> "max"
                },
                onChange = { onSetNoiseStrength(it.toInt()) },
            )
            TuningSlider(
                label = "Voice quality",
                value = state.prefs.voiceBitrate.toFloat(),
                range = 8_000f..128_000f,
                // A ceiling, not a promise: the core drops below it under loss.
                display = "${state.prefs.voiceBitrate / 1000} kbps max",
                onChange = { onSetVoiceBitrate((it / 1000).toInt() * 1000) },
            )

            Spacer(Modifier.height(16.dp))
            HorizontalDivider()
            Spacer(Modifier.height(8.dp))

            // ── Text ──────────────────────────────────────────────────────────
            // Its own section, not part of Appearance: a skin does not carry it.
            SectionTitle("Text")
            Text(
                "Scales text everywhere in DoubleSlash. Themes and skins do not change this.",
                style = MaterialTheme.typography.labelSmall,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
            )
            TuningSlider(
                label = "Font size",
                value = state.prefs.fontScalePercent.toFloat(),
                range = AppSettings.FONT_SCALE_PERCENT_MIN.toFloat()..AppSettings.FONT_SCALE_PERCENT_MAX.toFloat(),
                // Stops every 5% between the ends: (200 − −50) / 5 − 1.
                steps = 49,
                display = fontScaleLabel(state.prefs.fontScalePercent),
                onChange = { raw ->
                    val stepped = (kotlin.math.round(raw / 5f).toInt() * 5)
                        .coerceIn(AppSettings.FONT_SCALE_PERCENT_MIN, AppSettings.FONT_SCALE_PERCENT_MAX)
                    if (stepped != state.prefs.fontScalePercent) onSetFontScale(stepped)
                },
            )

            Spacer(Modifier.height(16.dp))
            HorizontalDivider()
            Spacer(Modifier.height(8.dp))

            // ── Date and time ─────────────────────────────────────────────
            SectionTitle("Date")
            Text(
                "Chat times use this phone's clock. Military is the previous 24-hour stamp.",
                style = MaterialTheme.typography.labelSmall,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
            )
            DateTimeFormats.all.forEach { format ->
                Row(
                    modifier = Modifier
                        .fillMaxWidth()
                        .clickable { onSetTimeFormat(format.id) }
                        .padding(vertical = 2.dp),
                    verticalAlignment = Alignment.CenterVertically,
                ) {
                    RadioButton(
                        selected = state.prefs.timeFormat == format.id,
                        onClick = { onSetTimeFormat(format.id) },
                    )
                    Column(Modifier.padding(start = 8.dp)) {
                        Text(format.label)
                        Text(
                            DateTimeFormats.format(System.currentTimeMillis() / 1000.0, format.id),
                            style = MaterialTheme.typography.labelSmall,
                            color = MaterialTheme.colorScheme.onSurfaceVariant,
                        )
                    }
                }
            }

            Spacer(Modifier.height(16.dp))
            HorizontalDivider()
            Spacer(Modifier.height(8.dp))

            // ── Appearance ────────────────────────────────────────────────
            SectionTitle("Appearance")
            AppearanceSection(
                theme = state.prefs.theme,
                skinJson = state.prefs.skin,
                onSetTheme = onSetTheme,
                onSetSkin = onSetSkin,
            )

            Spacer(Modifier.height(16.dp))
            HorizontalDivider()
            Spacer(Modifier.height(8.dp))

            // ── Network ───────────────────────────────────────────────────
            SectionTitle("Network")
            Text("Supernodes", style = MaterialTheme.typography.titleSmall)
            Spacer(Modifier.height(4.dp))
            Text(
                "The servers that relay your traffic and host rooms. Added by " +
                    "accepting an invite.",
                style = MaterialTheme.typography.labelSmall,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
            )
            Spacer(Modifier.height(8.dp))
            if (state.supernodeInfo.isEmpty()) {
                Text(
                    "None yet.",
                    style = MaterialTheme.typography.bodyMedium,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
            }
            state.supernodeInfo.forEach { node ->
                Row(
                    modifier = Modifier.fillMaxWidth().padding(vertical = 8.dp),
                    verticalAlignment = Alignment.CenterVertically,
                ) {
                    Column(Modifier.weight(1f)) {
                        Text(node.displayName, style = MaterialTheme.typography.bodyLarge)
                        Text(
                            // A cluster presents as one node; saying how many
                            // members it has is what distinguishes "one server"
                            // from "three that fail over".
                            if (node.clusterMembers.size > 1) {
                                "cluster of ${node.clusterMembers.size}"
                            } else {
                                node.peerId.take(16)
                            },
                            style = MaterialTheme.typography.labelSmall,
                            color = MaterialTheme.colorScheme.onSurfaceVariant,
                        )
                    }
                    TextButton(onClick = { onOpenPortal(node) }) { Text("Portal") }
                    TextButton(onClick = { confirmRemoveNode = node }) {
                        Text("Remove", color = MaterialTheme.colorScheme.error)
                    }
                }
            }

            Spacer(Modifier.height(16.dp))
            HorizontalDivider()
            Spacer(Modifier.height(8.dp))

            // ── Privacy ───────────────────────────────────────────────────
            SectionTitle("Privacy")
            Text("Chat history", style = MaterialTheme.typography.titleSmall)
            Spacer(Modifier.height(4.dp))
            Text(
                "Stored on this device only. Deleting does not remove anything " +
                    "from your peers - they keep their own copies.",
                style = MaterialTheme.typography.labelSmall,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
            )
            Spacer(Modifier.height(8.dp))
            Row {
                TextButton(onClick = { onTrimHistory(30) }) { Text("Trim over 30 days") }
                TextButton(onClick = { confirmPurge = true }) {
                    Text("Delete all", color = MaterialTheme.colorScheme.error)
                }
            }

            Spacer(Modifier.height(16.dp))
            HorizontalDivider()
            Spacer(Modifier.height(8.dp))

            // ── About ─────────────────────────────────────────────────────
            SectionTitle("About")
            val legalContext = LocalContext.current
            TextButton(onClick = { Legal.openUrl(legalContext, Legal.PRIVACY_URL) }) {
                Text("Privacy policy")
            }
            TextButton(onClick = { Legal.openUrl(legalContext, Legal.TERMS_URL) }) {
                Text("Terms of use")
            }
            TextButton(onClick = { showLicenses = true }) {
                Text("Third-party licenses")
            }
            Spacer(Modifier.height(8.dp))
            Text(
                "core $version",
                style = MaterialTheme.typography.labelSmall,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
            )
            Text(
                "Fingerprint ${state.identity.fingerprint.take(23)}",
                style = MaterialTheme.typography.labelSmall,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
            )
        }
    }

    if (showLicenses) LicenseViewer(onDismiss = { showLicenses = false })

    if (showAvatarEditor) {
        AvatarEditorDialog(
            onPreview = onPreviewAvatar,
            onDismiss = { showAvatarEditor = false },
            onSave = { json ->
                showAvatarEditor = false
                onSetAvatarConfig(json)
            },
        )
    }

    confirmRemoveNode?.let { node ->
        AlertDialog(
            onDismissRequest = { confirmRemoveNode = null },
            title = { Text("Remove ${node.displayName}?") },
            text = {
                Text(
                    "You will stop relaying through it and lose access to the " +
                        "rooms it hosts. Rooms stay in your list, so re-adding " +
                        "it later finds them again.",
                )
            },
            confirmButton = {
                TextButton(onClick = {
                    confirmRemoveNode = null
                    onRemoveSupernode(node.peerId)
                }) { Text("Remove", color = MaterialTheme.colorScheme.error) }
            },
            dismissButton = {
                TextButton(onClick = { confirmRemoveNode = null }) { Text("Cancel") }
            },
        )
    }

    if (confirmLock) {
        LockIdentityDialog(
            stayUnlocked = state.stayUnlocked,
            onDismiss = { confirmLock = false },
            onConfirm = {
                confirmLock = false
                onLock()
            },
        )
    }

    if (confirmPurge) {
        AlertDialog(
            onDismissRequest = { confirmPurge = false },
            title = { Text("Delete all messages?") },
            text = {
                Text(
                    "Every message on this device is removed. Your peers keep " +
                        "their copies, and this cannot be undone.",
                )
            },
            confirmButton = {
                TextButton(onClick = {
                    confirmPurge = false
                    onPurgeHistory()
                }) { Text("Delete all", color = MaterialTheme.colorScheme.error) }
            },
            dismissButton = {
                TextButton(onClick = { confirmPurge = false }) { Text("Cancel") }
            },
        )
    }
}

/**
 * Edit the identicon.
 *
 * A handful of the core's knobs, not all of them: grid size and the three
 * colour switches are what visibly change the picture, and the rest
 * (island connectivity, hue stepping, background lightness) are refinements
 * that need a bigger screen to be worth exposing.
 *
 * The preview is rendered by the core through `avatar.svg`, so what is shown
 * is exactly what peers will draw.
 */
@Composable
private fun AvatarEditorDialog(
    onPreview: suspend (String) -> AvatarArt?,
    onDismiss: () -> Unit,
    onSave: (String) -> Unit,
) {
    var grid by remember { mutableStateOf(16f) }
    var shadeMode by remember { mutableStateOf(1f) }
    var dualHue by remember { mutableStateOf(false) }
    var islands by remember { mutableStateOf(true) }
    var preview by remember { mutableStateOf<AvatarArt?>(null) }

    // Grid must be even: the pattern is mirrored about the vertical centre.
    val configJson = remember(grid, shadeMode, dualHue, islands) {
        val evenGrid = (grid.toInt() / 2) * 2
        "{\"grid\":$evenGrid,\"shade_mode\":${shadeMode.toInt()}," +
            "\"dual_hue\":$dualHue,\"islands\":$islands}"
    }

    LaunchedEffect(configJson) { preview = onPreview(configJson) }

    AlertDialog(
        onDismissRequest = onDismiss,
        title = { Text("Your avatar") },
        text = {
            Column {
                preview?.let { art ->
                    Box(Modifier.fillMaxWidth(), contentAlignment = Alignment.Center) {
                        Avatar(art, Modifier.size(96.dp))
                    }
                    Spacer(Modifier.height(16.dp))
                }

                Text("Detail", style = MaterialTheme.typography.labelMedium)
                Slider(
                    value = grid,
                    onValueChange = { grid = it },
                    valueRange = 8f..32f,
                    steps = 11,
                )

                Text("Shading", style = MaterialTheme.typography.labelMedium)
                Slider(
                    value = shadeMode,
                    onValueChange = { shadeMode = it },
                    valueRange = 1f..3f,
                    steps = 1,
                )

                Row(
                    modifier = Modifier
                        .fillMaxWidth()
                        .clickable { dualHue = !dualHue },
                    verticalAlignment = Alignment.CenterVertically,
                ) {
                    Checkbox(checked = dualHue, onCheckedChange = { dualHue = it })
                    Text("Two colours")
                }
                Row(
                    modifier = Modifier
                        .fillMaxWidth()
                        .clickable { islands = !islands },
                    verticalAlignment = Alignment.CenterVertically,
                ) {
                    Checkbox(checked = islands, onCheckedChange = { islands = it })
                    Text("Colour each shape")
                }
            }
        },
        confirmButton = { TextButton(onClick = { onSave(configJson) }) { Text("Save") } },
        dismissButton = { TextButton(onClick = onDismiss) { Text("Cancel") } },
    )
}

/** Signed percent for the font-size slider: "0%", "-50%", "+200%". */
private fun fontScaleLabel(percent: Int): String =
    if (percent > 0) "+$percent%" else "$percent%"

/** A labelled slider that shows the value it is about to set. */
@Composable
private fun TuningSlider(
    label: String,
    value: Float,
    range: ClosedFloatingPointRange<Float>,
    display: String,
    onChange: (Float) -> Unit,
    steps: Int = 0,
) {
    Row(verticalAlignment = Alignment.CenterVertically) {
        Text(label, style = MaterialTheme.typography.bodyMedium, modifier = Modifier.weight(1f))
        Text(
            display,
            style = MaterialTheme.typography.labelSmall,
            color = MaterialTheme.colorScheme.onSurfaceVariant,
        )
    }
    Slider(
        value = value.coerceIn(range.start, range.endInclusive),
        onValueChange = onChange,
        valueRange = range,
        steps = steps,
    )
}

@Composable
private fun SettingSwitch(
    title: String,
    subtitle: String,
    checked: Boolean,
    onCheckedChange: (Boolean) -> Unit,
) {
    Row(
        modifier = Modifier
            .fillMaxWidth()
            .clickable { onCheckedChange(!checked) }
            .padding(vertical = 8.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Column(Modifier.weight(1f)) {
            Text(title, style = MaterialTheme.typography.bodyLarge)
            Text(
                subtitle,
                style = MaterialTheme.typography.labelSmall,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
            )
        }
        Switch(checked = checked, onCheckedChange = onCheckedChange)
    }
}

// ── Files ──────────────────────────────────────────────────────────────────

/**
 * Ask before downloading. Declining sends nothing back — the offer simply
 * goes unanswered, which is what the protocol does too.
 */
@Composable
private fun FileOfferDialog(
    offer: FileOffer,
    onAccept: () -> Unit,
    onReject: () -> Unit,
) {
    AlertDialog(
        onDismissRequest = onReject,
        title = { Text(if (offer.isRoom) "File shared in a room" else "Incoming file") },
        text = {
            Column {
                Text(offer.name, style = MaterialTheme.typography.bodyLarge)
                Spacer(Modifier.height(4.dp))
                Text(
                    formatSize(offer.size),
                    style = MaterialTheme.typography.labelMedium,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
            }
        },
        confirmButton = { TextButton(onClick = onAccept) { Text("Accept") } },
        dismissButton = { TextButton(onClick = onReject) { Text("Decline") } },
    )
}

/**
 * Offer to copy a finished download somewhere the user can reach.
 *
 * The file is already saved in app storage; this is the copy out to their own
 * documents, so dismissing loses nothing but the shortcut.
 */
@Composable
private fun SaveFilePrompt(
    saved: SavedFile,
    onSave: (android.net.Uri) -> Unit,
    onDismiss: () -> Unit,
) {
    val createDocument = rememberLauncherForActivityResult(
        ActivityResultContracts.CreateDocument("*/*"),
    ) { uri -> if (uri != null) onSave(uri) else onDismiss() }

    AlertDialog(
        onDismissRequest = onDismiss,
        title = { Text("File received") },
        text = { Text("${saved.name} finished downloading. Save a copy?") },
        confirmButton = {
            TextButton(onClick = { createDocument.launch(saved.name) }) { Text("Save") }
        },
        dismissButton = { TextButton(onClick = onDismiss) { Text("Not now") } },
    )
}

/** Byte counts the way the desktop's `format_byte_size` renders them. */
private fun formatSize(bytes: Long): String = when {
    bytes >= 1024L * 1024 * 1024 -> "%.1f GB".format(bytes / (1024.0 * 1024 * 1024))
    bytes >= 1024L * 1024 -> "%.1f MB".format(bytes / (1024.0 * 1024))
    bytes >= 1024L -> "%.1f KB".format(bytes / 1024.0)
    else -> "$bytes bytes"
}

// ── Dialogs ────────────────────────────────────────────────────────────────

@Composable
private fun InviteContactDialog(
    roomName: String,
    contacts: List<Peer>,
    onPick: (Peer) -> Unit,
    onDismiss: () -> Unit,
) {
    AlertDialog(
        onDismissRequest = onDismiss,
        title = { Text("Invite Contact to Room") },
        text = {
            Column(Modifier.heightIn(max = 320.dp).verticalScroll(rememberScrollState())) {
                Text(
                    "A link for $roomName, bound to the contact you pick. It is copied when you choose them.",
                    style = MaterialTheme.typography.bodySmall,
                )
                Spacer(Modifier.height(8.dp))
                contacts.forEach { peer ->
                    TextButton(
                        onClick = { onPick(peer) },
                        modifier = Modifier.fillMaxWidth(),
                    ) {
                        Text(peer.label, modifier = Modifier.fillMaxWidth())
                    }
                }
            }
        },
        confirmButton = {},
        dismissButton = { TextButton(onClick = onDismiss) { Text("Cancel") } },
    )
}

@Composable
private fun InviteDialog(url: String, onDismiss: () -> Unit) {
    val clipboard = androidx.compose.ui.platform.LocalClipboardManager.current

    AlertDialog(
        onDismissRequest = onDismiss,
        title = { Text("Invite created") },
        text = {
            Column {
                Text(
                    "Single use. Send it over a channel you already trust.",
                    style = MaterialTheme.typography.bodySmall,
                )
                Spacer(Modifier.height(12.dp))
                Text(url, style = MaterialTheme.typography.bodySmall)
            }
        },
        confirmButton = {
            TextButton(onClick = {
                clipboard.setText(androidx.compose.ui.text.AnnotatedString(url))
                onDismiss()
            }) { Text("Copy") }
        },
        dismissButton = { TextButton(onClick = onDismiss) { Text("Close") } },
    )
}

@Composable
private fun AcceptInviteDialog(onDismiss: () -> Unit, onAccept: (String) -> Unit) {
    var url by remember { mutableStateOf("") }

    AlertDialog(
        onDismissRequest = onDismiss,
        title = { Text("Accept an invite") },
        text = {
            OutlinedTextField(
                value = url,
                onValueChange = { url = it },
                label = { Text("Invite link") },
                singleLine = false,
                maxLines = 4,
            )
        },
        confirmButton = {
            TextButton(onClick = { onAccept(url) }, enabled = url.isNotBlank()) { Text("Accept") }
        },
        dismissButton = { TextButton(onClick = onDismiss) { Text("Cancel") } },
    )
}

// ── Jump to current ────────────────────────────────────────────────────────

/**
 * How far from the newest message counts as "away", in list items.
 *
 * Small deliberately. The button exists so that reading back never hides an
 * arriving message, and a threshold much larger than this lets several land
 * unseen before anything says so.
 */
private const val JUMP_TO_CURRENT_AFTER_ITEMS = 3

/**
 * True while [listState] is scrolled far enough from the newest message that
 * an arriving message would land unseen below the fold.
 *
 * `derivedStateOf` rather than a plain read: scroll position changes every
 * frame while a finger is down, and recomposing the whole chat screen on each
 * of those would cost far more than the one boolean anyone actually reads.
 */
@Composable
private fun rememberScrolledAwayFromLatest(listState: LazyListState): State<Boolean> =
    remember(listState) {
        derivedStateOf {
            val info = listState.layoutInfo
            val last = info.visibleItemsInfo.lastOrNull() ?: return@derivedStateOf false
            info.totalItemsCount - 1 - last.index >= JUMP_TO_CURRENT_AFTER_ITEMS
        }
    }

/**
 * Keeps [listState] sitting on the newest message, and reports whether the
 * reader has moved off it - the one boolean [JumpToCurrentButton] needs.
 *
 * Three separate things move a chat list, and each wants different handling:
 *
 *  - **History arriving.** The screen opens with an empty list and the store
 *    answers a moment later, so the first populated frame is the one that
 *    decides where the reader starts. It gets the instant `scrollToItem`,
 *    because a launched effect runs before that frame's measure pass: an
 *    animated scroll would size itself from the layout it can still see, which
 *    is the empty one, and settle back at the top of the history.
 *  - **A message arriving.** Followed only while the reader is on the end.
 *    Snatching the view down mid-sentence is the thing the jump button exists
 *    to prevent.
 *  - **The viewport shrinking**, which on a phone means the keyboard. A
 *    LazyColumn holds its *top* anchor across a resize, so without this the
 *    newest message slides below the fold by exactly the keyboard's height.
 *
 * Whether to follow is remembered rather than measured when it is needed,
 * because the resize changes the very geometry a measured answer would be read
 * from: once the keyboard is up, a list that was pinned to the end looks
 * identical to one the reader had deliberately scrolled away from.
 */
/** What a chat list needs to stay on its newest message. */
private class PinnedToLatest(
    /** True while the reader has moved off the newest message. */
    val scrolledAway: State<Boolean>,
    /**
     * Put the reader back on the newest message and resume following it.
     *
     * The jump button's action, and also what sending does: posting a message
     * is as plain a statement that you want to see the end of the conversation
     * as tapping the button is, wherever you had scrolled to before.
     */
    val jumpToLatest: () -> Unit,
)

@Composable
private fun rememberPinnedToLatest(
    listState: LazyListState,
    conversationKey: String,
    itemCount: Int,
    /**
     * Id of the newest message, or null while there are none.
     *
     * The arrival trigger, and deliberately not [itemCount]: `chat.history`
     * answers with at most one page, so a conversation past
     * `chat_store::PAGE_SIZE` holds at exactly that many messages forever.
     * Every new message slides the window - oldest off the front, newest onto
     * the end - and a count keyed effect never fires again, which is every
     * long conversation silently losing its follow.
     */
    latestKey: String?,
): PinnedToLatest {
    val scrolledAway = rememberScrolledAwayFromLatest(listState)
    val scope = rememberCoroutineScope()

    // Both reset per conversation: a newly opened room or peer starts on its
    // own newest message, wherever the last one was left.
    var anchored by remember(conversationKey) { mutableStateOf(false) }
    var following by remember(conversationKey) { mutableStateOf(true) }

    // The two halves below each move `following` one way only, which is what
    // keeps a reading taken mid-resize from doing damage: the keyboard's inset
    // animates over many frames, and during those the list is genuinely
    // clipped, so anything that could disarm on geometry alone would disarm
    // exactly when the pinning is needed.

    // Coming to rest on the end re-arms - that covers the jump button and a
    // drag back down to the bottom without either having to say so, and a
    // stray re-arm only pins a list that wanted pinning anyway.
    LaunchedEffect(listState, conversationKey) {
        snapshotFlow { listState.isScrollInProgress }.collect { moving ->
            if (!moving && !scrolledAway.value) following = true
        }
    }

    // Only the reader's own gesture disarms. Programmatic scrolls raise no
    // drag interaction, so this cannot be tripped by our own pinning.
    LaunchedEffect(listState, conversationKey) {
        listState.interactionSource.interactions.collect { interaction ->
            if (interaction !is DragInteraction.Stop &&
                interaction !is DragInteraction.Cancel
            ) {
                return@collect
            }
            // A fling carries on well past the finger, so where the reader
            // meant to leave the list is only knowable once it comes to rest.
            snapshotFlow { listState.isScrollInProgress }.first { !it }
            if (scrolledAway.value) following = false
        }
    }

    LaunchedEffect(listState, conversationKey, latestKey) {
        if (latestKey == null || itemCount == 0) return@LaunchedEffect
        if (!anchored) {
            listState.scrollToItem(itemCount - 1)
            anchored = true
        } else if (following) {
            // Let the arrival be laid out before animating to it. This effect
            // runs ahead of the frame's measure pass, so right now the list is
            // still the old one: already at the bottom of it, with the new
            // message not placed yet. `animateScrollToItem` asks to scroll
            // forward, gets nothing back because the old content has nowhere
            // left to go, and takes that as its cue to give up - leaving the
            // view one message short of the end, which is exactly the scroll
            // the reader then has to do by hand.
            withFrameNanos {}
            listState.animateScrollToItem(itemCount - 1)
        }
    }

    // The keyboard, and anything else that resizes the list. `drop(1)` skips
    // the initial measurement, which is the anchoring effect's job, not a
    // resize. Instant rather than animated for every frame of it: the inset
    // animates over a good fraction of a second, and a scroll animation
    // racing that one would visibly lag behind the keyboard on the way up.
    LaunchedEffect(listState, conversationKey) {
        snapshotFlow { listState.layoutInfo.viewportSize.height }
            .drop(1)
            .collect {
                val last = listState.layoutInfo.totalItemsCount - 1
                if (following && last >= 0) listState.scrollToItem(last)
            }
    }

    return PinnedToLatest(
        // Mid-drag the measured answer is the honest one - the button should
        // show as soon as the end leaves the screen, not when the finger comes
        // up. Keyed on the conversation as well as the list: a new key hands
        // `following` a fresh state object, and a lambda remembered only
        // against `listState` would go on reading the outgoing one.
        scrolledAway = remember(listState, conversationKey) {
            derivedStateOf { scrolledAway.value || !following }
        },
        jumpToLatest = {
            following = true
            scope.launch {
                val last = listState.layoutInfo.totalItemsCount - 1
                if (last >= 0) listState.animateScrollToItem(last)
            }
        },
    )
}

/**
 * The affordance back to the newest message, shown over the bottom of a chat
 * list while the reader has scrolled away from it.
 *
 * Pairs with suppressing auto-scroll: a list that keeps snapping to the end
 * needs no such button, and one that stops snapping without offering it
 * strands the reader.
 */
@Composable
private fun BoxScope.JumpToCurrentButton(visible: Boolean, onClick: () -> Unit) {
    AnimatedVisibility(
        visible = visible,
        enter = fadeIn(),
        exit = fadeOut(),
        modifier = Modifier.align(Alignment.BottomCenter).padding(bottom = 12.dp),
    ) {
        FilledTonalButton(onClick = onClick) {
            Icon(Icons.Filled.KeyboardArrowDown, contentDescription = null)
            Spacer(Modifier.width(6.dp))
            Text("Jump to current")
        }
    }
}
