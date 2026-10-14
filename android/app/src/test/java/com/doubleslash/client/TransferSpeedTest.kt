package com.doubleslash.client

import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test

/** Mirrors `TransferSpeed` in the Rust core: same window, same smoothing. */
class TransferSpeedTest {
    private val twoMib = 2L * 1024L * 1024L

    @Test
    fun firstSampleOpensTheWindow() {
        val speed = LiveTransfer().observe(0f, twoMib, 0L, "notes.txt")
        assertEquals(0.0, speed.bytesPerSec, 0.0)
        assertEquals("", speed.speedLabel)
        assertEquals("0%", speed.progressCaption())
        val early = speed.observe(0.25f, twoMib, 100L, "notes.txt")
        assertEquals("", early.speedLabel)
        assertEquals("25%", early.progressCaption())
    }

    @Test
    fun rateIsFileBytesOverTheWindow() {
        val opened = LiveTransfer().observe(0f, twoMib, 0L, "notes.txt")
        // Half of 2 MiB in one second is 1 MiB/s.
        val speed = opened.observe(0.5f, twoMib, 1000L, "notes.txt")
        assertEquals(1_048_576.0, speed.bytesPerSec, 1.0)
        assertEquals("1.0 MB/s", speed.speedLabel)
        assertEquals("50% · 1.0 MB/s", speed.progressCaption())
        assertEquals("notes.txt", speed.name)
    }

    @Test
    fun aQuietWindowClearsTheRate() {
        val moving = LiveTransfer()
            .observe(0f, twoMib, 0L, "")
            .observe(0.5f, twoMib, 1000L, "")
        val stalled = moving.observe(0.5f, twoMib, 2000L, "")
        assertEquals(0.0, stalled.bytesPerSec, 0.0)
        assertEquals("", stalled.speedLabel)
        assertEquals("50%", stalled.progressCaption())
    }

    @Test
    fun aRewindKeepsTheLastRate() {
        val moving = LiveTransfer()
            .observe(0f, twoMib, 0L, "")
            .observe(0.5f, twoMib, 1000L, "")
        val rewound = moving.observe(0.25f, twoMib, 1100L, "")
        assertEquals(1_048_576.0, rewound.bytesPerSec, 1.0)
        assertEquals("1.0 MB/s", rewound.speedLabel)
    }

    @Test
    fun unknownSizeNeverInventsARate() {
        val speed = LiveTransfer()
            .observe(0f, 0L, 0L, "")
            .observe(0.5f, 0L, 1000L, "")
        assertEquals(0.0, speed.bytesPerSec, 0.0)
        assertEquals("", speed.speedLabel)
    }

    @Test
    fun labelsUseTheDesktopUnits() {
        assertEquals("", formatTransferSpeed(0.0))
        assertEquals("512 B/s", formatTransferSpeed(512.0))
        assertEquals("1.5 KB/s", formatTransferSpeed(1536.0))
        assertEquals("1.0 MB/s", formatTransferSpeed(1024.0 * 1024.0))
        assertEquals("1.5 GB/s", formatTransferSpeed(1.5 * 1024.0 * 1024.0 * 1024.0))
    }

    @Test
    fun fileNameIsTheLastPathSegment() {
        assertEquals("c.txt", transferFileName("a/b\\c.txt"))
        assertEquals("notes.txt", transferFileName("notes.txt"))
        assertTrue(transferFileName("").isEmpty())
    }
}
