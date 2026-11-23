package com.doubleslash.client

import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test

class LegalTest {

    @Test
    fun `terms version is a positive integer`() {
        // A zero here would skip the accept gate: acceptedTermsVersion
        // defaults to 0, and Home is shown when accepted >= TERMS_VERSION.
        assertTrue(Legal.TERMS_VERSION > 0)
        assertEquals(
            "https://github.com/DoubleSlashSpace/DoubleSlash/blob/develop/PRIVACY.md",
            Legal.PRIVACY_URL,
        )
    }
}
