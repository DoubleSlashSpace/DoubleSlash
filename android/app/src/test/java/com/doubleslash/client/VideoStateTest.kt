package com.doubleslash.client

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

/**
 * Covers which peers the voice rail and call card treat as streaming and
 * watched.
 *
 * Camera state arrives keyed by the sender's base64 `public_id`, padded or not
 * depending on the path that carried it, while a direct call is keyed by the
 * hex peer-store id. Getting either bridge wrong greys out "Watch video" for a
 * peer whose camera is plainly on.
 */
class VideoStateTest {

    private val bob = Peer(peerId = "0a1b2c", identityPub = "Qm9iS2V5==")

    @Test
    fun `a room member's camera is found whichever padding it arrived with`() {
        val state = AppState(streamingPeers = setOf("Qm9iS2V5"))
        assertTrue(state.cameraOn("Qm9iS2V5=="))
        assertTrue(state.cameraOn("Qm9iS2V5"))
    }

    @Test
    fun `a direct call's hex id is bridged to the announced public id`() {
        val state = AppState(peers = listOf(bob), streamingPeers = setOf("Qm9iS2V5"))
        assertTrue(state.cameraOn("0a1b2c"))
    }

    @Test
    fun `an unknown peer with no announcement has no camera`() {
        val state = AppState(peers = listOf(bob), streamingPeers = setOf("Qm9iS2V5"))
        assertFalse(state.cameraOn("ffffff"))
    }

    @Test
    fun `a streamer still on a roster stays streaming after we leave voice`() {
        // Leaving voice keeps our SFU membership, and rejoining sends no join
        // the streamer would replay their camera-on to: forgetting it here is
        // what made a live stream look idle after a leave and rejoin.
        val state = AppState(
            streamingPeers = setOf("Qm9iS2V5"),
            roomVoiceRosters = mapOf("node:room" to listOf("Qm9iS2V5==")),
            voiceRoom = null,
        )
        assertEquals(setOf("Qm9iS2V5"), state.streamingStillPresent())
    }

    @Test
    fun `a text-only roster keeps a streamer too`() {
        val state = AppState(
            streamingPeers = setOf("Qm9iS2V5"),
            roomTextRosters = mapOf("node:room" to listOf("Qm9iS2V5")),
        )
        assertEquals(setOf("Qm9iS2V5"), state.streamingStillPresent())
    }

    @Test
    fun `a streamer who left every roster is retired`() {
        // They sent no camera-off on the way out; departure is the only signal.
        val state = AppState(
            streamingPeers = setOf("Qm9iS2V5", "QWxpY2U"),
            roomVoiceRosters = mapOf("node:room" to listOf("QWxpY2U=")),
        )
        assertEquals(setOf("QWxpY2U"), state.streamingStillPresent())
    }

    @Test
    fun `a direct call keeps its peer's camera without any roster`() {
        val state = AppState(
            peers = listOf(bob),
            call = CallState(peerId = "0a1b2c", peerLabel = "Bob", phase = CallPhase.ACTIVE),
            streamingPeers = setOf("Qm9iS2V5"),
        )
        assertEquals(setOf("Qm9iS2V5"), state.streamingStillPresent())
        assertEquals(emptySet<String>(), state.copy(call = null).streamingStillPresent())
    }

    @Test
    fun `watching ignores padding`() {
        val state = AppState(watchedVideo = listOf("Qm9iS2V5=="))
        assertTrue(state.watching("Qm9iS2V5"))
        assertFalse(state.watching("someone-else"))
    }
}
