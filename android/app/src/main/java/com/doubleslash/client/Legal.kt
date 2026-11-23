package com.doubleslash.client

import android.content.Context
import android.content.Intent
import android.net.Uri
import android.util.Log

/**
 * Public legal URLs for the in-app terms and privacy links.
 */
object Legal {
    const val TERMS_VERSION = 1

    const val PRIVACY_URL =
        "https://github.com/DoubleSlashSpace/DoubleSlash/blob/develop/PRIVACY.md"
    const val TERMS_URL =
        "https://github.com/DoubleSlashSpace/DoubleSlash/blob/develop/TERMS.md"

    fun openUrl(context: Context, url: String) {
        val intent = Intent(Intent.ACTION_VIEW, Uri.parse(url))
        runCatching { context.startActivity(intent) }
            .onFailure { Log.w(TAG, "could not open $url", it) }
    }

    private const val TAG = "Legal"
}
