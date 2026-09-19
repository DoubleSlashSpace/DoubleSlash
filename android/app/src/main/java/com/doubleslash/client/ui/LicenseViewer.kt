package com.doubleslash.client.ui

import android.webkit.WebResourceRequest
import android.webkit.WebResourceResponse
import android.webkit.WebView
import android.webkit.WebViewClient
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.produceState
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.viewinterop.AndroidView
import androidx.compose.ui.window.Dialog
import androidx.compose.ui.window.DialogProperties
import com.doubleslash.client.Legal
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext

/** Reads every packaged notice, including nested supplements, without a network connection. */
@Composable
fun LicenseViewer(onDismiss: () -> Unit) {
    val context = LocalContext.current
    var selected by remember { mutableStateOf<String?>(null) }
    val notices by produceState<List<String>?>(null) {
        value = withContext(Dispatchers.IO) {
            fun walk(path: String): List<String> = context.assets.list(path).orEmpty().flatMap { name ->
                val child = "$path/$name"
                if (context.assets.list(child).orEmpty().isNotEmpty()) walk(child)
                else if (name.endsWith(".gz") || name.endsWith(".zip")) emptyList()
                else listOf(child)
            }
            walk("licenses").sorted()
        }
    }
    val document by produceState<String?>(null, selected) {
        value = null
        val path = selected ?: return@produceState
        value = withContext(Dispatchers.IO) {
            runCatching {
                val text = context.assets.open(path).bufferedReader().use { it.readText() }
                if (path.endsWith(".html", ignoreCase = true)) text
                else "<html><head><meta name=\"viewport\" content=\"width=device-width, initial-scale=1\"></head>" +
                    "<body><pre style=\"white-space:pre-wrap;overflow-wrap:anywhere\">" +
                    android.text.TextUtils.htmlEncode(text) + "</pre></body></html>"
            }.getOrElse { "<p>This notice could not be read.</p>" }
        }
    }
    Dialog(onDismissRequest = onDismiss, properties = DialogProperties(usePlatformDefaultWidth = false)) {
        Surface(Modifier.fillMaxSize()) {
            Column {
                TextButton(onClick = { if (selected == null) onDismiss() else selected = null }) {
                    Text(if (selected == null) "Close licenses" else "All licenses")
                }
                val path = selected
                if (path == null) {
                    Text("Third-party licenses")
                    when {
                        notices == null -> Text("Loading notices…")
                        notices!!.isEmpty() -> Text("No license notices were packaged in this build.")
                        else -> LazyColumn(Modifier.weight(1f)) {
                            items(notices!!) { notice ->
                                TextButton(onClick = { selected = notice }) {
                                    Text(notice.removePrefix("licenses/"))
                                }
                            }
                        }
                    }
                } else {
                    Text(path.removePrefix("licenses/"))
                    val html = document
                    if (html == null) Text("Loading notice…")
                    else AndroidView(
                        modifier = Modifier.weight(1f),
                        factory = { WebView(it).apply {
                            settings.javaScriptEnabled = false
                            settings.allowFileAccess = false
                            settings.allowContentAccess = false
                            settings.blockNetworkLoads = true
                            webViewClient = object : WebViewClient() {
                                override fun shouldOverrideUrlLoading(view: WebView, request: WebResourceRequest): Boolean {
                                    if (request.isForMainFrame && request.url.scheme == "https") {
                                        Legal.openUrl(view.context, request.url.toString())
                                    }
                                    return true
                                }

                                override fun shouldInterceptRequest(view: WebView, request: WebResourceRequest) =
                                    WebResourceResponse("text/plain", "UTF-8", "".byteInputStream())
                            }
                        } },
                        update = { view ->
                            if (view.tag != html) {
                                view.tag = html
                                view.loadDataWithBaseURL(null, html, "text/html", "UTF-8", null)
                            }
                        },
                        onRelease = { it.destroy() },
                        onReset = null,
                    )
                }
            }
        }
    }
}
