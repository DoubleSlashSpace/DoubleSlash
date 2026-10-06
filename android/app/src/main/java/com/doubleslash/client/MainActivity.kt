package com.doubleslash.client

import android.content.Intent
import android.net.Uri
import android.os.Build
import android.os.Bundle
import androidx.activity.ComponentActivity
import androidx.activity.compose.setContent
import androidx.activity.enableEdgeToEdge
import androidx.compose.foundation.isSystemInDarkTheme
import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.unit.Density
import androidx.lifecycle.ViewModelProvider
import com.doubleslash.client.ui.AppRoot
import com.doubleslash.client.ui.DoubleSlashTheme

class MainActivity : ComponentActivity() {

    private val viewModel: AppViewModel by lazy {
        ViewModelProvider(this)[AppViewModel::class.java]
    }

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        enableEdgeToEdge()

        setContent {
            // Collected here rather than inside the theme so a change repaints
            // the whole tree, including the system bars.
            val state by viewModel.state.collectAsState()
            // Text scale sits outside the theme on purpose: it is a device
            // setting, not a skin. Density stays the system density so layout
            // dp does not change; only sp text grows or shrinks. The system
            // font scale is kept and multiplied.
            val systemDensity = LocalDensity.current
            val textScale = (100 + state.prefs.fontScalePercent
                .coerceIn(AppSettings.FONT_SCALE_PERCENT_MIN, AppSettings.FONT_SCALE_PERCENT_MAX)) / 100f
            CompositionLocalProvider(
                LocalDensity provides Density(
                    density = systemDensity.density,
                    fontScale = systemDensity.fontScale * textScale,
                ),
            ) {
                DoubleSlashTheme(
                    darkTheme = when (state.prefs.theme) {
                        AppSettings.THEME_DARK -> true
                        AppSettings.THEME_LIGHT -> false
                        else -> isSystemInDarkTheme()
                    },
                    skinJson = state.prefs.skin,
                ) {
                    AppRoot(viewModel = viewModel)
                }
            }
        }

        handleIncomingCall(intent)
        handleInviteIntent(intent)
        viewModel.handleRoomAlertIntent(intent)
    }

    override fun onStart() {
        super.onStart()
        viewModel.onBroughtToFront()
    }

    /**
     * The activity is `singleTask`, so a second invite link while the app is
     * already open arrives here instead of through `onCreate`.
     */
    override fun onNewIntent(intent: Intent) {
        super.onNewIntent(intent)
        // Without this the activity keeps returning the intent it launched
        // with, so anything reading `getIntent()` later sees a stale link.
        setIntent(intent)
        handleIncomingCall(intent)
        handleInviteIntent(intent)
        viewModel.handleRoomAlertIntent(intent)
    }

    /**
     * Incoming-call notification / lock-screen full-screen intent.
     *
     * Wake the display only for a ring, not for ordinary launches: showing
     * over the lock screen is a calling-app privilege, not a default.
     */
    private fun handleIncomingCall(intent: Intent?) {
        val ringing = intent?.action == IncomingCallNotifier.ACTION_SHOW ||
            intent?.action == IncomingCallNotifier.ACTION_ANSWER
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.O_MR1) {
            setShowWhenLocked(ringing)
            setTurnScreenOn(ringing)
        }
        viewModel.handleIncomingCallIntent(intent)
    }

    /**
     * Hand an invite link to the core.
     *
     * The link usually arrives before the identity is unlocked — tapping an
     * invite is a common way to open the app for the first time — so the view
     * model holds it until there is a core to give it to.
     */
    private fun handleInviteIntent(intent: Intent?) {
        if (intent?.action != Intent.ACTION_VIEW) return
        val uri = intent.data ?: return
        if (!isInviteLink(uri)) return

        viewModel.acceptInvite(uri.toString())
    }

    /**
     * Invites are minted as https links — chat clients only auto-link http(s),
     * and an https link still lands somewhere useful when the app is missing —
     * but every scheme form is accepted too. The core normalizes all of them;
     * this only decides what is worth handing over.
     */
    private fun isInviteLink(uri: Uri): Boolean {
        val scheme = uri.scheme?.lowercase() ?: return false
        if (scheme in APP_SCHEMES) return true
        if (scheme != "https" && scheme != "http") return false
        if (uri.host?.lowercase()?.removePrefix("www.") != INVITE_HOST) return false
        // The payload rides in the fragment, which never reaches the server.
        val path = uri.path?.trimEnd('/').orEmpty()
        return path in INVITE_PATHS && !uri.fragment.isNullOrEmpty()
    }

    private companion object {
        val APP_SCHEMES = setOf("doubleslash", "d")
        const val INVITE_HOST = "doubleslash.space"
        val INVITE_PATHS = setOf("/i", "/r")
    }
}
