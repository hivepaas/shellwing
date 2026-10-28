package com.hivepaas.shellwing.ui

import androidx.compose.foundation.ExperimentalFoundationApi
import androidx.compose.foundation.background
import androidx.compose.foundation.combinedClickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.CheckCircle
import androidx.compose.material.icons.outlined.Cloud
import androidx.compose.material.icons.outlined.CloudDone
import androidx.compose.material.icons.outlined.CloudOff
import androidx.compose.material.icons.outlined.CloudSync
import androidx.compose.material.icons.outlined.PersonAddAlt
import androidx.compose.material.icons.outlined.SyncProblem
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.Button
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.DropdownMenu
import androidx.compose.material3.DropdownMenuItem
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.Icon
import androidx.compose.material3.LinearProgressIndicator
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.SegmentedButton
import androidx.compose.material3.SegmentedButtonDefaults
import androidx.compose.material3.SingleChoiceSegmentedButtonRow
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.produceState
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.input.KeyboardType
import androidx.compose.ui.text.input.PasswordVisualTransformation
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import com.hivepaas.shellwing.AppModel
import com.hivepaas.shellwing.core.Connected
import com.hivepaas.shellwing.core.Pairing
import com.hivepaas.shellwing.core.Place
import com.hivepaas.shellwing.core.Shellwing
import com.hivepaas.shellwing.core.SyncStatus
import com.hivepaas.shellwing.said
import com.hivepaas.shellwing.tr
import com.hivepaas.shellwing.trN
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.delay
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext

/**
 * Settings › Sync: where the vault syncs, and joining one another device set
 * up, as the desktop's Settings › Sync and the iOS app's SyncView. WebDAV,
 * S3 and Git: Android gives an app no folder of the user's to write in by
 * its path, which a synced folder needs.
 */
@Composable
fun SyncScreen(model: AppModel, onBack: () -> Unit) {
    val status = model.sync
    val pairing = status?.pairing
    // The recovery key of a vault this phone just put at the place, shown
    // until Done: the place is set by then, which would show Syncing.
    var started by remember { mutableStateOf<String?>(null) }
    // A pairing that ended (joined, refused, expired) is said until Done;
    // leaving says it was seen.
    DisposableEffect(Unit) {
        onDispose {
            val p = model.sync?.pairing
            if (p != null && p !is Pairing.Waiting && p !is Pairing.Compare) model.core?.cancelJoin()
        }
    }
    SubScreen(tr("Sync"), onBack) {
        val key = started
        when {
            key != null -> Started(key) { started = null }
            status?.place != null && pairing == null -> Syncing(model, status)
            // Under the pairing, so a cancel finds the place still typed.
            else -> SetUp(model, pairing) { started = it }
        }
    }
}

/** Seconds since 1970, again every half minute: what "ago" counts from. */
@Composable
private fun now(): Long {
    val now by produceState(System.currentTimeMillis() / 1000) {
        while (true) {
            delay(30_000)
            value = System.currentTimeMillis() / 1000
        }
    }
    return now
}

/** "2 minutes ago", for a sync: the desktop's words. */
private fun ago(last: ULong?, now: Long): String? {
    val s = (now - (last ?: return null).toLong()).coerceAtLeast(0)
    return when {
        s < 60 -> tr("just now")
        s < 120 -> tr("a minute ago")
        s < 3600 -> trN((s / 60).toInt(), "{n} minute ago", "{n} minutes ago")
        s < 7200 -> tr("an hour ago")
        s < 86400 -> trN((s / 3600).toInt(), "{n} hour ago", "{n} hours ago")
        else -> trN((s / 86400).toInt(), "{n} day ago", "{n} days ago")
    }
}

/** The recovery key of the vault this phone put at the place. */
@Composable
private fun Started(key: String, onDone: () -> Unit) {
    val context = LocalContext.current
    Column(Modifier.fillMaxSize().verticalScroll(rememberScrollState()).padding(16.dp), verticalArrangement = Arrangement.spacedBy(12.dp)) {
        Heading(tr("Recovery key"))
        Text(tr("This phone syncs there now. The recovery key opens the vault on your other devices: write it down."), color = Dusk.soft)
        Text(
            key, fontFamily = FontFamily.Monospace, fontSize = 16.sp,
            modifier = Modifier.fillMaxWidth().clip(RoundedCornerShape(12.dp)).background(Dusk.field).padding(14.dp),
        )
        Row(horizontalArrangement = Arrangement.spacedBy(12.dp)) {
            OutlinedButton(onClick = { copy(context, tr("Recovery key"), key, secret = true) }) { Text(tr("Copy")) }
            Button(onClick = onDone) { Text(tr("Done")) }
        }
    }
}

/** Where it syncs, how it went, and the devices asking to join. */
@Composable
private fun Syncing(model: AppModel, status: SyncStatus) {
    var stopping by remember { mutableStateOf(false) }
    Column(Modifier.fillMaxSize().verticalScroll(rememberScrollState()).padding(16.dp), verticalArrangement = Arrangement.spacedBy(12.dp)) {
        Labeled(tr("Place"), status.said ?: "")
        Row(verticalAlignment = Alignment.CenterVertically) {
            Text(tr("Last synced"), modifier = Modifier.weight(1f))
            if (status.syncing) {
                CircularProgressIndicator(Modifier.size(18.dp), strokeWidth = 2.dp)
            } else {
                Text(ago(status.last, now()) ?: tr("not yet"), color = if (status.last == null) Dusk.muted else Dusk.soft)
            }
        }
        status.error?.let { Text(it, color = Dusk.bad, fontSize = 14.sp) }
        OutlinedButton(onClick = { model.core?.syncNow() }, enabled = !status.syncing) { Text(tr("Sync now")) }
        Hint(tr("End-to-end encrypted: the place holds only what the vault's key opens. It syncs every minute while shellwing is open, and when something changes."))

        if (status.requests.isNotEmpty()) {
            HorizontalDivider(color = Dusk.line)
            Heading(tr("Devices asking"))
            for (r in status.requests) {
                Column(
                    Modifier.fillMaxWidth().clip(RoundedCornerShape(12.dp)).background(Dusk.surface).padding(14.dp),
                    verticalArrangement = Arrangement.spacedBy(8.dp),
                ) {
                    Text(tr("{device} asks to join", "device" to r.device), fontSize = 16.sp)
                    Text(
                        tr("Approve only if it shows {code}. It gets everything the vault holds: passwords, keys, snippets and hosts.", "code" to r.code),
                        fontSize = 14.sp, color = Dusk.soft,
                    )
                    Row {
                        TextButton(onClick = { model.core?.refuse(r.id); model.refreshSync() }) { Text(tr("Refuse"), color = Dusk.bad) }
                        Spacer(Modifier.weight(1f))
                        Button(onClick = { model.core?.approve(r.id); model.refreshSync() }) { Text(tr("Approve")) }
                    }
                }
            }
        }
        HorizontalDivider(color = Dusk.line)
        TextButton(onClick = { stopping = true }) { Text(tr("Stop syncing"), color = Dusk.bad) }
    }
    if (stopping) {
        AlertDialog(
            onDismissRequest = { stopping = false },
            title = { Text(tr("Stop syncing on this phone?")) },
            text = { Text(tr("The vault stays here, and the copy at the place stays for your other devices.")) },
            confirmButton = {
                TextButton(onClick = {
                    stopping = false
                    try {
                        model.core?.stopSync()
                    } catch (e: Exception) {
                        model.error = e.said()
                    }
                    model.refreshSync()
                }) { Text(tr("Stop syncing"), color = Dusk.bad) }
            },
            dismissButton = { TextButton(onClick = { stopping = false }) { Text(tr("Cancel")) } },
        )
    }
}

@Composable
private fun Labeled(label: String, value: String) {
    Row {
        Text(label, modifier = Modifier.weight(1f))
        Text(value, color = Dusk.soft, textAlign = TextAlign.End, modifier = Modifier.weight(2f))
    }
}

/** This phone's request to join, while another device answers. */
@Composable
private fun Pairing(model: AppModel, pairing: Pairing) {
    Column(
        Modifier.fillMaxSize().padding(24.dp),
        horizontalAlignment = Alignment.CenterHorizontally,
        verticalArrangement = Arrangement.spacedBy(18.dp),
    ) {
        when (pairing) {
            is Pairing.Waiting -> {
                CircularProgressIndicator()
                Text(tr("Waiting for a device that syncs there…"), fontSize = 18.sp)
                Text(tr("Open shellwing on it: it asks whether to let this phone in."), color = Dusk.soft, textAlign = TextAlign.Center)
            }
            is Pairing.Compare -> {
                Text(tr("{device} answered", "device" to pairing.device), fontSize = 18.sp)
                Text(
                    pairing.code, fontFamily = FontFamily.Monospace, fontSize = 34.sp,
                    modifier = Modifier.clip(RoundedCornerShape(12.dp)).background(Dusk.field).padding(horizontal = 18.dp, vertical = 10.dp),
                )
                Text(tr("Approve on {device} if it shows the same code.", "device" to pairing.device), color = Dusk.soft, textAlign = TextAlign.Center)
            }
            is Pairing.Joined -> {
                Icon(Icons.Filled.CheckCircle, null, tint = Dusk.ok, modifier = Modifier.size(44.dp))
                Text(tr("Joined: this phone syncs there now."), fontSize = 18.sp, textAlign = TextAlign.Center)
            }
            is Pairing.Refused -> Text(tr("The other device refused to let this one in."), color = Dusk.bad, textAlign = TextAlign.Center)
            is Pairing.Expired -> Text(
                tr("No device let this one in within 10 minutes. Ask again, or use the recovery key."),
                color = Dusk.bad, textAlign = TextAlign.Center,
            )
        }
        val pending = pairing is Pairing.Waiting || pairing is Pairing.Compare
        OutlinedButton(onClick = { model.core?.cancelJoin(); model.refreshSync() }) {
            Text(if (pending) tr("Cancel") else tr("Done"))
        }
    }
}

private enum class Kind(val label: String) { WebDav("WebDAV"), S3("S3"), Git("Git") }

/** Choosing a place, connecting, and joining what is there; while this phone asks to join, how that goes. */
@Composable
private fun SetUp(model: AppModel, pairing: Pairing?, onStarted: (String) -> Unit) {
    val scope = rememberCoroutineScope()
    var kind by rememberSaveable { mutableStateOf(Kind.WebDav) }
    var url by rememberSaveable { mutableStateOf("") }
    var user by rememberSaveable { mutableStateOf("") }
    var password by remember { mutableStateOf("") }
    var endpoint by rememberSaveable { mutableStateOf("") }
    var region by rememberSaveable { mutableStateOf("") }
    var bucket by rememberSaveable { mutableStateOf("") }
    var prefix by rememberSaveable { mutableStateOf("") }
    var accessKey by rememberSaveable { mutableStateOf("") }
    var secretKey by remember { mutableStateOf("") }
    var repository by rememberSaveable { mutableStateOf("") }
    var branch by rememberSaveable { mutableStateOf("") }
    var token by remember { mutableStateOf("") }
    var tokenUser by rememberSaveable { mutableStateOf("") }
    var busy by remember { mutableStateOf(false) }
    var problem by remember { mutableStateOf<String?>(null) }
    /** Another device's vault was there: how to join it. */
    var other by remember { mutableStateOf<Pair<Place, Boolean>?>(null) }
    var joinKey by remember { mutableStateOf("") }
    var joinPassphrase by remember { mutableStateOf("") }
    if (pairing != null) {
        Pairing(model, pairing)
        return
    }

    /** A call that reaches the place, off the main thread. */
    fun reach(work: (Shellwing) -> Unit) {
        val core = model.core ?: run { problem = model.problem; return }
        busy = true
        problem = null
        scope.launch {
            try {
                withContext(Dispatchers.IO) { work(core) }
            } catch (e: Exception) {
                problem = e.said()
            }
            busy = false
            model.refreshSync()
        }
    }

    val place: Pair<Place, String?> = when (kind) {
        Kind.WebDav -> Place.WebDav(url.trim(), user) to password.ifEmpty { null }
        Kind.S3 -> Place.S3(endpoint.trim(), region.trim(), bucket.trim(), prefix.trim(), accessKey.trim()) to secretKey
        Kind.Git -> Place.Git(repository.trim(), branch.trim(), tokenUser.trim()) to token
    }
    val ready = when (kind) {
        Kind.WebDav -> url.isNotBlank()
        Kind.S3 -> endpoint.isNotBlank() && bucket.isNotBlank() && accessKey.isNotBlank() && secretKey.isNotBlank()
        Kind.Git -> repository.trim().startsWith("https://") && token.isNotEmpty()
    }

    Column(Modifier.fillMaxSize().verticalScroll(rememberScrollState()).padding(16.dp), verticalArrangement = Arrangement.spacedBy(12.dp)) {
        if (busy) LinearProgressIndicator(Modifier.fillMaxWidth())
        val join = other
        when {
            join != null -> {
                val (p, hasPassphrase) = join
                Heading(tr("Join"))
                Text(tr("Another device's vault is there. Join it to get its hosts, keys, passwords and snippets on this phone."), color = Dusk.soft)
                Button(onClick = { model.core?.askToJoin(p); model.refreshSync() }, enabled = !busy) {
                    Text(tr("Pair with a device that syncs there"))
                }
                HorizontalDivider(color = Dusk.line)
                Heading(tr("Recovery key"))
                PlainField(tr("XXXX-XXXX-…, 13 groups of 4"), joinKey, mono = true) { joinKey = it }
                OutlinedButton(onClick = { reach { it.joinWithRecoveryKey(p, joinKey) } }, enabled = !busy && joinKey.length >= 20) {
                    Text(tr("Join with the recovery key"))
                }
                if (hasPassphrase) {
                    HorizontalDivider(color = Dusk.line)
                    Heading(tr("Passphrase"))
                    Secret(tr("The passphrase set on the other device"), joinPassphrase) { joinPassphrase = it }
                    OutlinedButton(onClick = { reach { it.joinWithPassphrase(p, joinPassphrase) } }, enabled = !busy && joinPassphrase.isNotEmpty()) {
                        Text(tr("Join with the passphrase"))
                    }
                }
            }
            else -> {
                Heading(tr("Sync the vault"))
                SingleChoiceSegmentedButtonRow(Modifier.fillMaxWidth()) {
                    Kind.entries.forEachIndexed { i, k ->
                        SegmentedButton(
                            selected = kind == k, onClick = { kind = k },
                            shape = SegmentedButtonDefaults.itemShape(i, Kind.entries.size),
                        ) { Text(k.label) }
                    }
                }
                when (kind) {
                    Kind.WebDav -> {
                        PlainField(tr("Where"), url, type = KeyboardType.Uri, placeholder = "https://cloud.example.com/remote.php/dav/files/me/shellwing") { url = it }
                        PlainField(tr("User"), user) { user = it }
                        Secret(tr("Password or app password"), password) { password = it }
                    }
                    Kind.S3 -> {
                        PlainField(tr("Endpoint: https://s3.eu-west-1.amazonaws.com"), endpoint, type = KeyboardType.Uri) { endpoint = it }
                        PlainField(tr("Region (empty: auto)"), region) { region = it }
                        PlainField(tr("Bucket"), bucket) { bucket = it }
                        PlainField(tr("Folder in the bucket (optional)"), prefix) { prefix = it }
                        PlainField(tr("Access key"), accessKey) { accessKey = it }
                        Secret(tr("Secret key"), secretKey) { secretKey = it }
                    }
                    Kind.Git -> {
                        PlainField(tr("Where"), repository, type = KeyboardType.Uri, placeholder = "https://github.com/you/vault.git") { repository = it }
                        PlainField(tr("Branch"), branch) { branch = it }
                        Secret(tr("Token"), token) { token = it }
                        PlainField(tr("The token's user"), tokenUser) { tokenUser = it }
                    }
                }
                Button(onClick = {
                    val (p, secret) = place
                    val core = model.core ?: return@Button
                    busy = true
                    problem = null
                    scope.launch {
                        try {
                            when (val found = withContext(Dispatchers.IO) { core.connectSync(p, secret) }) {
                                is Connected.Started -> onStarted(found.recoveryKey)
                                is Connected.Other -> other = p to found.passphrase
                            }
                        } catch (e: Exception) {
                            problem = e.said()
                        }
                        busy = false
                        model.refreshSync()
                    }
                }, enabled = ready && !busy) { Text(tr("Connect")) }
                Hint(
                    if (kind == Kind.Git) {
                        tr("A private repository on GitHub, GitLab, Gitea or your own server, by its https:// address, with a token that may push there (on GitHub, a fine-grained token with Contents: read and write). Each change is a commit. The token is kept in this phone's vault.")
                    } else {
                        tr("The password is kept in this phone's vault, never in the place.")
                    },
                )
            }
        }
        problem?.let { Text(it, color = Dusk.bad) }
    }
}

@Composable
private fun Secret(label: String, value: String, onChange: (String) -> Unit) {
    OutlinedTextField(
        value = value, onValueChange = onChange, label = { Text(label) }, singleLine = true,
        visualTransformation = PasswordVisualTransformation(),
        keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.Password, autoCorrectEnabled = false),
        modifier = Modifier.fillMaxWidth(),
    )
}

/** How the sync stands at a glance: its icon, color and words. */
private data class Look(val icon: ImageVector, val color: Color, val said: String)

private fun look(s: SyncStatus?, now: Long): Look {
    if (s?.place == null) return Look(Icons.Outlined.CloudOff, Dusk.muted, tr("Not synced"))
    s.requests.firstOrNull()?.let { return Look(Icons.Outlined.PersonAddAlt, Dusk.amber, tr("{device} asks to join", "device" to it.device)) }
    s.error?.let { return Look(Icons.Outlined.SyncProblem, Dusk.bad, tr("Cannot sync: {e}", "e" to it)) }
    if (s.syncing) return Look(Icons.Outlined.CloudSync, Dusk.accent, tr("Syncing…"))
    val when_ = ago(s.last, now) ?: return Look(Icons.Outlined.Cloud, Dusk.soft, tr("Not synced"))
    return Look(Icons.Outlined.CloudDone, Dusk.ok, tr("Synced · {when}", "when" to when_))
}

/**
 * The sync at a glance, in the host list's top bar as the design has it:
 * green when synced, blue while it syncs, amber when a device asks to join,
 * red when it cannot sync, crossed out when off. A tap opens Sync; held,
 * Sync now.
 */
@OptIn(ExperimentalFoundationApi::class)
@Composable
fun SyncButton(model: AppModel, onOpen: () -> Unit) {
    val l = look(model.sync, now())
    var menu by remember { mutableStateOf(false) }
    Box {
        Box(
            Modifier.size(48.dp).clip(CircleShape)
                .combinedClickable(onClick = onOpen, onLongClick = { menu = true })
                .semantics { contentDescription = l.said },
            contentAlignment = Alignment.Center,
        ) { Icon(l.icon, null, tint = l.color) }
        DropdownMenu(expanded = menu, onDismissRequest = { menu = false }) {
            if (model.sync?.place != null) {
                DropdownMenuItem(text = { Text(tr("Sync now")) }, onClick = { menu = false; model.core?.syncNow() })
            }
            DropdownMenuItem(text = { Text(tr("Sync")) }, onClick = { menu = false; onOpen() })
        }
    }
}

/** The sync's state in a line, for Settings. */
@Composable
fun syncSummary(s: SyncStatus?): String = look(s, now()).said
