package com.conquerd.client.ui

import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.text.input.KeyboardType
import androidx.compose.ui.text.input.PasswordVisualTransformation
import androidx.compose.ui.unit.dp
import androidx.compose.ui.window.Dialog
import androidx.compose.ui.window.DialogProperties
import com.conquerd.client.DoubleSlashCore
import com.conquerd.client.AppSettings
import com.conquerd.client.ok
import com.conquerd.client.errorText
import com.conquerd.client.stringOrEmpty
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import kotlinx.serialization.json.*
import java.io.File

@Composable
fun BackupButton(unlocked: Boolean, enabled: Boolean = true) {
    var show by remember { mutableStateOf(false) }
    TextButton(enabled = enabled, onClick = { show = true }) { Text(if (unlocked) "Devices & Backups" else "Restore backup or load another identity") }
    if (show) BackupDialog(unlocked = unlocked, onDismiss = { show = false })
}

/** The encrypted file is staged in app cache; decrypted imports stay in Rust's
 * private restore directory until the user approves the verified preview. */
@Composable
fun BackupDialog(unlocked: Boolean, onDismiss: () -> Unit) {
    val context = LocalContext.current
    val core = remember { DoubleSlashCore.get(context) }
    val scope = rememberCoroutineScope()
    var page by remember { mutableStateOf("choose") }
    var busy by remember { mutableStateOf(false) }
    var error by remember { mutableStateOf("") }
    var password by remember { mutableStateOf("") }
    var confirmation by remember { mutableStateOf("") }
    var attachments by remember { mutableStateOf(true) }
    var summary by remember { mutableStateOf<JsonObject?>(null) }
    var token by remember { mutableStateOf("") }
    var profiles by remember { mutableStateOf(JsonArray(emptyList())) }
    var fileUri by remember { mutableStateOf<android.net.Uri?>(null) }

    fun applyReply(reply: JsonObject, next: String) {
        if (!reply.ok) { error = reply.errorText ?: "Operation failed"; return }
        summary = reply["summary"] as? JsonObject
        token = reply.stringOrEmpty("token")
        password = ""; confirmation = ""; page = next
    }

    val exportPicker = rememberLauncherForActivityResult(ActivityResultContracts.CreateDocument("application/octet-stream")) { uri ->
        if (uri != null) { fileUri = uri; page = "export" }
    }
    val importPicker = rememberLauncherForActivityResult(ActivityResultContracts.OpenDocument()) { uri ->
        if (uri != null) { fileUri = uri; page = "import" }
    }
    LaunchedEffect(Unit) {
        if (!unlocked) {
            val reply = core.backupCommand(buildJsonObject { put("cmd", "profile.list") })
            profiles = reply["profiles"] as? JsonArray ?: JsonArray(emptyList())
        }
    }

    Dialog(onDismissRequest = { if (!busy) onDismiss() }, properties = DialogProperties(dismissOnBackPress = false, dismissOnClickOutside = false)) {
        Surface(shape = MaterialTheme.shapes.large) {
            Column(Modifier.padding(24.dp).verticalScroll(rememberScrollState()), verticalArrangement = Arrangement.spacedBy(12.dp)) {
                Text("Devices & Backups", style = MaterialTheme.typography.titleLarge)
                Text("Restore your identity and local data with one encrypted file and its backup password. DoubleSlash cannot recover a lost backup password.")
                if (page == "choose") {
                    if (unlocked) {
                        Button(onClick = { exportPicker.launch("DoubleSlash-${java.time.LocalDate.now()}.dbackup") }) { Text("Create encrypted backup") }
                        Text("To restore or load another identity, disconnect and lock first, then open this wizard from the unlock screen.")
                    } else {
                        Button(onClick = { importPicker.launch(arrayOf("application/octet-stream", "*/*")) }) { Text("Restore from backup") }
                        profiles.forEach { value ->
                            val profile = value.jsonObject
                            TextButton(enabled = !busy, onClick = {
                                busy = true; error = ""
                                scope.launch {
                                    val reply = core.backupCommand(buildJsonObject { put("cmd", "profile.select"); put("profile", profile.stringOrEmpty("profile")) })
                                    applyReply(reply, "restored"); busy = false
                                }
                            }) { Text("Load identity ${profile.stringOrEmpty("public_id").take(12)}…") }
                        }
                    }
                    Text("Moving between devices: export here, transfer the file, then restore on the other device. Quit the old device before connecting the new one. Simultaneous use and ongoing sync are not supported yet.")
                }
                if (page in listOf("export", "import", "preview")) {
                    if (page == "export") {
                        Row {
                            Checkbox(checked = attachments, onCheckedChange = { attachments = it }, enabled = !busy)
                            Text("Include available attachment files", Modifier.padding(top = 12.dp))
                        }
                    }
                    if (page == "preview") {
                        Text("Verified identity: ${summary?.stringOrEmpty("public_id")}")
                        Text("${summary?.get("messages")} messages · ${summary?.get("attachments")} attachments\nMissing attachments: ${summary?.get("missing_attachments")}")
                        Text("Restoring creates a separate profile. Your existing identities are preserved. Choose a new passphrase to unlock this device.")
                    }
                    OutlinedTextField(value = password, onValueChange = { password = it }, enabled = !busy,
                        label = { Text(if (page == "preview") "New local passphrase" else "Backup password") },
                        visualTransformation = PasswordVisualTransformation(), keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.Password), singleLine = true)
                    if (page != "import") {
                        OutlinedTextField(value = confirmation, onValueChange = { confirmation = it }, enabled = !busy,
                            label = { Text("Confirm (at least 12 characters)") }, visualTransformation = PasswordVisualTransformation(),
                            keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.Password), singleLine = true)
                    }
                    Button(enabled = !busy && password.isNotEmpty() && (page == "import" || (password.length >= 12 && password == confirmation)), onClick = {
                        busy = true; error = ""
                        val operation = page
                        val secret = password
                        val includeFiles = attachments
                        scope.launch {
                            try {
                                if (operation == "preview") {
                                    val reply = core.backupCommand(buildJsonObject { put("cmd", "backup.restore"); put("token", token); put("local_password", secret) })
                                    applyReply(reply, "restored")
                                } else {
                                    val uri = checkNotNull(fileUri)
                                    val reply = withContext(Dispatchers.IO) {
                                        val staged = File.createTempFile("doubleslash-", ".dbackup", context.cacheDir)
                                        try {
                                            if (operation == "import") {
                                                checkNotNull(context.contentResolver.openInputStream(uri)).use { input -> staged.outputStream().use { input.copyTo(it) } }
                                            } else { check(staged.delete()) }
                                            val response = core.backupCommand(buildJsonObject {
                                                put("cmd", if (operation == "export") "backup.export" else "backup.inspect")
                                                put("path", staged.absolutePath); put("password", secret); put("attachments", includeFiles)
                                                if (operation == "export") put("android_settings", AppSettings(context).backupValues())
                                            })
                                            if (operation == "export" && response.ok) {
                                                checkNotNull(context.contentResolver.openOutputStream(uri, "wt")).use { output -> staged.inputStream().use { it.copyTo(output) } }
                                            }
                                            response
                                        } finally { staged.delete() }
                                    }
                                    applyReply(reply, if (operation == "export") "exported" else "preview")
                                }
                            } catch (failure: Exception) { error = failure.message ?: "Could not read or write the backup file" }
                            finally { busy = false }
                        }
                    }) { Text(if (page == "export") "Create and verify backup" else if (page == "import") "Verify and preview" else "Restore into new profile") }
                }
                if (page == "exported") Text("Backup saved and verified. ${summary?.get("messages")} messages, ${summary?.get("attachments")} attachments. Missing attachments: ${summary?.get("missing_attachments")}.")
                if (page == "restored") Text("Profile selected. Close this wizard and unlock with its passphrase. Quit any other device using this identity before connecting.")
                if (busy) { CircularProgressIndicator(); Text("Processing and verifying… Large backups can take a few minutes.") }
                if (error.isNotEmpty()) Text(error, color = MaterialTheme.colorScheme.error)
                TextButton(enabled = !busy, onClick = {
                    busy = true
                    scope.launch { core.backupCommand(buildJsonObject { put("cmd", "backup.cancel") }); onDismiss() }
                }) { Text("Close") }
            }
        }
    }
}
