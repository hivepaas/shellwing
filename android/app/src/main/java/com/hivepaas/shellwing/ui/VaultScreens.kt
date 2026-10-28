package com.hivepaas.shellwing.ui

import android.content.ClipData
import android.content.ClipboardManager
import android.content.Intent
import android.os.Build
import android.os.PersistableBundle
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.ArrowBack
import androidx.compose.material.icons.filled.Add
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.Button
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.ExtendedFloatingActionButton
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.LinearProgressIndicator
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.Scaffold
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.material3.TopAppBar
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import com.hivepaas.shellwing.AppModel
import com.hivepaas.shellwing.SessionModel
import com.hivepaas.shellwing.confirmOwner
import com.hivepaas.shellwing.core.KeyRow
import com.hivepaas.shellwing.said
import com.hivepaas.shellwing.tr
import kotlinx.coroutines.launch

/** Puts [text] on the clipboard; [secret] keeps it out of Android's clipboard preview. */
fun copy(context: android.content.Context, label: String, text: String, secret: Boolean = false) {
    val clip = ClipData.newPlainText(label, text)
    if (secret && Build.VERSION.SDK_INT >= 33) {
        clip.description.extras = PersistableBundle().apply { putBoolean("android.content.extra.IS_SENSITIVE", true) }
    }
    context.getSystemService(ClipboardManager::class.java)?.setPrimaryClip(clip)
}

/** The SSH keys in the vault: made here, installed on hosts from a session. */
@Composable
fun KeysList(model: AppModel) {
    val context = LocalContext.current
    var keys by remember { mutableStateOf(model.core?.keys() ?: emptyList()) }
    var naming by remember { mutableStateOf(false) }
    Box(Modifier.fillMaxSize()) {
        Library(
            empty = if (keys.isEmpty()) {
                tr("No keys yet. Make one, then install it on a host from a session's menu: it signs in from then on, without a password.")
            } else null,
        ) {
            items(keys, key = { it.id }) { k ->
                Item(
                    k.name, "${k.kind} · ${k.fingerprint}", mono = true,
                    onClick = {
                        context.startActivity(
                            Intent.createChooser(Intent(Intent.ACTION_SEND).setType("text/plain").putExtra(Intent.EXTRA_TEXT, k.public), k.name),
                        )
                    },
                    onDelete = { model.core?.forget(k.id); keys = model.core?.keys() ?: emptyList() },
                    deleteTitle = tr("Delete {name}?", "name" to k.name),
                    deleteSays = tr("Hosts that sign in with it need another way in.") + " " + goesEverywhere(),
                    trailing = { TextButton(onClick = { copy(context, k.name, k.public) }) { Text(tr("Copy public key")) } },
                )
            }
        }
        ExtendedFloatingActionButton(
            onClick = { naming = true }, icon = { Icon(Icons.Filled.Add, null) }, text = { Text(tr("Make a key")) },
            containerColor = Dusk.accent, contentColor = Dusk.onAccent,
            modifier = Modifier.align(Alignment.BottomEnd).padding(16.dp),
        )
    }
    if (naming) {
        NewKeyDialog(onDone = { name ->
            naming = false
            if (name != null) {
                try {
                    model.core?.generateKey(name)
                } catch (e: Exception) {
                    model.error = e.said()
                }
                keys = model.core?.keys() ?: emptyList()
            }
        })
    }
}

@Composable
private fun NewKeyDialog(onDone: (String?) -> Unit) {
    var name by remember { mutableStateOf(Build.MODEL) }
    AlertDialog(
        onDismissRequest = { onDone(null) },
        title = { Text(tr("A new Ed25519 key")) },
        text = {
            Column(verticalArrangement = Arrangement.spacedBy(8.dp)) {
                Text(tr("Kept in the vault, encrypted, with your other secrets."), color = Dusk.muted)
                PlainField(tr("Name"), name) { name = it }
            }
        },
        confirmButton = { TextButton(onClick = { onDone(name.trim()) }, enabled = name.isNotBlank()) { Text(tr("Make")) } },
        dismissButton = { TextButton(onClick = { onDone(null) }) { Text(tr("Cancel")) } },
    )
}

@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun SubScreen(title: String, onBack: () -> Unit, content: @Composable () -> Unit) {
    Scaffold(
        topBar = {
            TopAppBar(
                navigationIcon = { IconButton(onClick = onBack) { Icon(Icons.AutoMirrored.Filled.ArrowBack, tr("Settings")) } },
                title = { Text(title) },
            )
        },
    ) { pad -> Box(Modifier.fillMaxSize().padding(pad)) { content() } }
}

/** Passwords and passphrases kept when they worked; forgotten, they are asked for again. */
@Composable
fun SecretsScreen(model: AppModel, onBack: () -> Unit) {
    var secrets by remember { mutableStateOf(model.core?.savedSecrets() ?: emptyList()) }
    SubScreen(tr("Saved passwords"), onBack) {
        Library(
            empty = if (secrets.isEmpty()) tr("Nothing saved yet. When a password typed in a session works, shellwing offers to keep it.") else null,
        ) {
            items(secrets, key = { it.id }) { s ->
                Item(
                    s.what, if (s.passphrase) tr("A key's passphrase") else tr("Password"),
                    onClick = {},
                    onDelete = { model.core?.forget(s.id); secrets = model.core?.savedSecrets() ?: emptyList() },
                    deleteTitle = tr("Forget {name}?", "name" to s.what),
                    deleteSays = tr("It is asked for again next time."),
                    deleteLabel = tr("Forget"),
                )
            }
        }
    }
}

/** The vault's recovery key, once the owner says it is them. */
@Composable
fun RecoveryKeyScreen(model: AppModel, onBack: () -> Unit) {
    val context = LocalContext.current
    var key by remember { mutableStateOf<String?>(null) }
    var problem by remember { mutableStateOf<String?>(null) }
    fun ask() {
        problem = null
        confirmOwner(context, tr("Show the vault's recovery key")) { ok ->
            if (ok) {
                key = model.core?.recoveryKey()
                if (key == null) problem = tr("The vault is not open.")
            } else {
                problem = tr("Not confirmed: the key stays hidden.")
            }
        }
    }
    LaunchedEffect(Unit) { ask() }
    SubScreen(tr("Recovery key"), onBack) {
        Column(Modifier.fillMaxWidth().padding(20.dp), horizontalAlignment = Alignment.CenterHorizontally) {
            val k = key
            if (k != null) {
                Text(
                    tr("Write it down and keep it apart from this phone. It opens the vault where nothing else can."),
                    textAlign = TextAlign.Center, color = Dusk.soft,
                )
                Spacer(Modifier.height(18.dp))
                for (row in k.split("-").chunked(3)) {
                    Row(horizontalArrangement = Arrangement.spacedBy(8.dp), modifier = Modifier.padding(bottom = 10.dp)) {
                        for (g in row) {
                            Box(
                                Modifier.weight(1f).heightIn(min = 40.dp).clip(RoundedCornerShape(8.dp)).background(Dusk.field)
                                    .border(1.dp, Dusk.fieldEdge, RoundedCornerShape(8.dp)),
                                contentAlignment = Alignment.Center,
                            ) { Text(g, fontFamily = FontFamily.Monospace, fontSize = 18.sp) }
                        }
                        repeat(3 - row.size) { Spacer(Modifier.weight(1f)) }
                    }
                }
                OutlinedButton(onClick = { copy(context, tr("Recovery key"), k, secret = true) }) { Text(tr("Copy")) }
            } else if (problem != null) {
                Text(problem!!, color = Dusk.bad)
                Spacer(Modifier.height(12.dp))
                Button(onClick = ::ask) { Text(tr("Try again")) }
            }
        }
    }
}

/** Puts a vault key's public line on the session's host: one kept, or one made now. */
@Composable
fun InstallKey(model: AppModel, session: SessionModel, onDone: (String?) -> Unit) {
    val scope = rememberCoroutineScope()
    var keys by remember { mutableStateOf(model.core?.keys() ?: emptyList()) }
    var name by remember { mutableStateOf(Build.MODEL) }
    var busy by remember { mutableStateOf(false) }
    var problem by remember { mutableStateOf<String?>(null) }

    fun install(k: KeyRow) {
        val handle = session.handle ?: run { problem = tr("Not connected."); return }
        busy = true
        problem = null
        scope.launch {
            try {
                val added = handle.installKey(k.public)
                onDone(
                    if (added) tr("{name} is on {host}: it signs in from now on.", "name" to k.name, "host" to session.spec)
                    else tr("{name} was on {host} already.", "name" to k.name, "host" to session.spec),
                )
            } catch (e: Exception) {
                problem = e.said()
                busy = false
            }
        }
    }

    Column(Modifier.padding(bottom = 24.dp)) {
        Heading(tr("Install a key"), Modifier.padding(16.dp))
        if (busy) LinearProgressIndicator(Modifier.fillMaxWidth())
        if (keys.isNotEmpty()) {
            Heading(tr("Keys in the vault"), Modifier.padding(start = 16.dp))
            LazyColumn(Modifier.heightIn(max = 280.dp)) {
                items(keys, key = { it.id }) { k -> Item(k.name, "${k.kind} · ${k.fingerprint}", mono = true, onClick = { if (!busy) install(k) }) }
            }
        }
        Column(Modifier.padding(16.dp), verticalArrangement = Arrangement.spacedBy(8.dp)) {
            Heading(tr("A new key"))
            PlainField(tr("Name"), name) { name = it }
            Button(enabled = !busy && name.isNotBlank(), onClick = {
                try {
                    val k = model.core?.generateKey(name.trim()) ?: return@Button
                    keys = model.core?.keys() ?: emptyList()
                    install(k)
                } catch (e: Exception) {
                    problem = e.said()
                }
            }) { Text(tr("Make an Ed25519 key and install it")) }
            Hint(tr("Kept in the vault, encrypted; only its public line goes to {host}.", "host" to session.spec))
            problem?.let { Text(it, color = Dusk.bad) }
        }
    }
}
