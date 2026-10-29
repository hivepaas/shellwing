package com.hivepaas.shellwing.ui

import android.annotation.SuppressLint
import android.content.Intent
import android.net.Uri
import android.os.PowerManager
import android.provider.Settings
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.foundation.lazy.items
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.ArrowBack
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.Slider
import androidx.compose.runtime.mutableFloatStateOf
import androidx.compose.runtime.saveable.rememberSaveable
import com.hivepaas.shellwing.core.KnownHostRow
import androidx.compose.material3.DropdownMenu
import androidx.compose.material3.DropdownMenuItem
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.Scaffold
import androidx.compose.material3.Switch
import androidx.compose.material3.Text
import androidx.compose.material3.TopAppBar
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import androidx.lifecycle.compose.LifecycleResumeEffect
import com.hivepaas.shellwing.AppLock
import com.hivepaas.shellwing.AppModel
import com.hivepaas.shellwing.BuildConfig
import com.hivepaas.shellwing.core.HostKeyWay
import com.hivepaas.shellwing.core.OnDisconnectWay
import com.hivepaas.shellwing.core.languages
import com.hivepaas.shellwing.said
import com.hivepaas.shellwing.tr
import com.hivepaas.shellwing.trN

/** Settings: the background, the lock, connections, the language. */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun SettingsScreen(model: AppModel, lock: AppLock, onLockNow: () -> Unit, onOpen: (Screen) -> Unit) {
    val context = LocalContext.current
    var settings by remember { mutableStateOf(model.core?.settings()) }
    val change: ((com.hivepaas.shellwing.core.Shellwing) -> Unit) -> Unit = { work ->
        model.core?.let {
            try {
                work(it)
            } catch (e: Exception) {
                model.error = e.said()
            }
            settings = it.settings()
        }
    }
    // Asked again each time the screen shows: the user may have changed it in Android's settings.
    var unrestricted by remember { mutableStateOf(true) }
    LifecycleResumeEffect(Unit) {
        unrestricted = context.getSystemService(PowerManager::class.java)
            ?.isIgnoringBatteryOptimizations(context.packageName) ?: true
        onPauseOrDispose {}
    }

    Scaffold(topBar = { TopAppBar(title = { Text(tr("Settings")) }) }) { pad ->
        Column(Modifier.fillMaxSize().padding(pad).verticalScroll(rememberScrollState())) {
            Heading(tr("In the background"), Modifier.padding(start = 16.dp, top = 8.dp))
            SwitchRow(
                tr("Keep sessions connected"),
                tr("While a session is open, a notification shows it and Android keeps it connected."),
                model.keepInBackground,
            ) { model.keepWhenAway(it) }
            Row(
                tr("Battery use"),
                if (unrestricted) {
                    tr("Unrestricted: Android leaves the sessions alone.")
                } else {
                    tr("Optimized: Android may close the sessions in the background. Tap to allow unrestricted use.")
                },
                subColor = if (unrestricted) Dusk.muted else Color(0xfffde68a),
            ) {
                @SuppressLint("BatteryLife")
                val ask = Intent(Settings.ACTION_REQUEST_IGNORE_BATTERY_OPTIMIZATIONS, Uri.parse("package:${context.packageName}"))
                val open = Intent(Settings.ACTION_IGNORE_BATTERY_OPTIMIZATION_SETTINGS)
                runCatching { context.startActivity(if (unrestricted) open else ask) }
            }

            HorizontalDivider(Modifier.padding(vertical = 8.dp))
            Heading(tr("Lock"), Modifier.padding(start = 16.dp))
            val choices = listOf(0, 1, 5, 15, 30, 60)
            Choice(
                tr("Lock shellwing when away"),
                choices.map { it to minutes(it) },
                lock.after,
            ) { lock.lockAfter(it) }
            if (lock.after > 0) Row(tr("Lock now"), null) { onLockNow() }

            HorizontalDivider(Modifier.padding(vertical = 8.dp))
            Heading(tr("Terminal"), Modifier.padding(start = 16.dp))
            settings?.let { s ->
                val themeName = remember(s.theme) { model.core?.themes()?.firstOrNull { it.id == s.theme }?.name ?: s.theme }
                Row(tr("Theme"), themeName) { onOpen(Screen.AppTheme) }
                TextSize(model)
                Choice(
                    tr("History"),
                    SCROLLBACKS.map { it to trN(it.toInt(), "{n} line", "{n} lines") },
                    s.scrollback,
                ) { v -> change { it.setScrollback(v) } }
            }
            Footer(tr("Sessions opened after a change take it. A pinch on the terminal sizes its text too; the size is this phone's own."))

            HorizontalDivider(Modifier.padding(vertical = 8.dp))
            Heading(tr("Connections"), Modifier.padding(start = 16.dp))
            settings?.let { s ->
                Choice(
                    tr("When a connection drops"),
                    listOf(
                        OnDisconnectWay.RECONNECT to tr("Reconnect"),
                        OnDisconnectWay.ASK to tr("Ask"),
                        OnDisconnectWay.CLOSE to tr("Close"),
                    ),
                    s.onDisconnect,
                ) { v -> change { it.setOnDisconnect(v) } }
                Choice(
                    tr("New host keys"),
                    listOf(
                        HostKeyWay.ASK to tr("Ask"),
                        HostKeyWay.ACCEPT_NEW to tr("Accept them"),
                        HostKeyWay.STRICT to tr("Refuse them"),
                    ),
                    s.hostKey,
                ) { v -> change { it.setHostKey(v) } }
                Choice(
                    tr("Keepalive"),
                    KEEPALIVES.map { it to if (it == 0uL) tr("None") else trN(it.toInt(), "Every {n} second", "Every {n} seconds") },
                    s.keepalive,
                ) { v -> change { it.setKeepalive(v) } }
            }
            Row(tr("Known hosts"), null) { onOpen(Screen.KnownHosts) }
            Footer(tr("A host's own, in its editor, wins. A key that changed is always refused: forget the host in Known hosts to accept its new one."))

            HorizontalDivider(Modifier.padding(vertical = 8.dp))
            Heading(tr("Vault"), Modifier.padding(start = 16.dp))
            val vault = remember { model.core?.vaultState() }
            if (vault?.ready == false && vault.problem != null) {
                Text(vault.problem!!, color = Dusk.bad, modifier = Modifier.padding(horizontal = 16.dp))
            } else {
                Row(tr("Saved passwords"), null) { onOpen(Screen.Secrets) }
                Row(tr("Recovery key"), null) { onOpen(Screen.RecoveryKey) }
                Row(tr("Sync"), syncSummary(model.sync)) { onOpen(Screen.Sync) }
            }

            HorizontalDivider(Modifier.padding(vertical = 8.dp))
            Heading(tr("Language"), Modifier.padding(start = 16.dp))
            var language by remember { mutableStateOf(settings?.language) }
            Choice(
                tr("Language"),
                listOf<Pair<String?, String>>(null to tr("As the phone")) + languages().map { it.tag to it.name },
                language,
            ) { tag ->
                change { it.setLanguage(tag) }
                language = tag
                model.reload()
            }

            HorizontalDivider(Modifier.padding(vertical = 8.dp))
            Heading(tr("About"), Modifier.padding(start = 16.dp))
            Row(tr("Version"), BuildConfig.VERSION_NAME) {}
            Row(tr("shellwing on GitHub"), null) {
                runCatching {
                    context.startActivity(Intent(Intent.ACTION_VIEW, Uri.parse("https://github.com/hivepaas/shellwing")))
                }
            }
            model.problem?.let {
                Heading(tr("Problem"), Modifier.padding(start = 16.dp, top = 8.dp))
                Text(it, color = Dusk.bad, modifier = Modifier.padding(16.dp))
            }
        }
    }
}

private val SCROLLBACKS = listOf(1_000uL, 5_000uL, 10_000uL, 50_000uL, 100_000uL)
private val KEEPALIVES = listOf(0uL, 15uL, 30uL, 60uL, 120uL)

/** The terminal's text size, this phone's own (a pinch on the terminal sets it too). */
@Composable
private fun TextSize(model: AppModel) {
    var size by remember { mutableFloatStateOf(model.prefs.getFloat(TerminalView.SIZE_KEY, TerminalView.DEFAULT_SIZE)) }
    Column(Modifier.fillMaxWidth().padding(horizontal = 16.dp, vertical = 8.dp)) {
        Row(verticalAlignment = Alignment.CenterVertically) {
            Text(tr("Text size"), fontSize = 16.sp, modifier = Modifier.weight(1f))
            Text("%s sp".format(if (size % 1f == 0f) size.toInt().toString() else size.toString()), fontSize = 14.sp, color = Dusk.muted)
        }
        Slider(
            value = size,
            onValueChange = { size = Math.round(it * 2) / 2f },
            onValueChangeFinished = { model.prefs.edit().putFloat(TerminalView.SIZE_KEY, size).apply() },
            // Half points, as a pinch makes them; no tick for each.
            valueRange = TerminalView.MIN_SIZE..TerminalView.MAX_SIZE,
        )
    }
}

@Composable
private fun Footer(text: String) {
    Text(text, fontSize = 13.sp, color = Dusk.muted, modifier = Modifier.padding(horizontal = 16.dp, vertical = 4.dp))
}

/** The terminal's theme, the app's: sessions opened after take it. */
@Composable
fun AppThemeScreen(model: AppModel, onBack: () -> Unit) {
    val themes = remember { model.core?.themes() ?: emptyList() }
    ThemePicker(themes, model.core?.settings()?.theme, appDefault = false, onBack = onBack) { id ->
        if (id != null) {
            try {
                model.core?.setTheme(id)
            } catch (e: Exception) {
                model.error = e.said()
            }
        }
        onBack()
    }
}

/** The hosts whose keys are known; one forgotten has its key asked about again next time. */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun KnownHostsScreen(model: AppModel, onBack: () -> Unit) {
    var problem by remember { mutableStateOf<String?>(null) }
    fun read(): List<KnownHostRow> = try {
        (model.core?.knownHosts() ?: emptyList()).also { problem = null }
    } catch (e: Exception) {
        problem = e.said()
        emptyList()
    }
    var rows by remember { mutableStateOf(read()) }
    var filter by rememberSaveable { mutableStateOf("") }
    Scaffold(
        topBar = {
            TopAppBar(
                navigationIcon = { IconButton(onClick = onBack) { Icon(Icons.AutoMirrored.Filled.ArrowBack, tr("Settings")) } },
                title = { Text(tr("Known hosts")) },
            )
        },
    ) { pad ->
        Box(Modifier.padding(pad)) {
            val q = filter.lowercase()
            val shown = rows.filter { q.isEmpty() || it.names.any { n -> n.lowercase().contains(q) } }
            Library(
                filter = filter, onFilter = { filter = it }, filterHint = tr("Filter by name"),
                empty = if (rows.isEmpty() && problem == null) tr("No host keys known yet: a host's is kept the first time you connect.") else null,
            ) {
                problem?.let { item { Text(it, color = Dusk.bad, modifier = Modifier.padding(16.dp)) } }
                items(shown, key = { it.line.toString() }) { r ->
                    val name = if (r.names.isEmpty()) tr("A hashed name") else r.names.joinToString(", ")
                    Item(
                        title = name, sub = "${r.kind} · ${r.fingerprint}", mono = true, onClick = {},
                        onDelete = {
                            try {
                                model.core?.forgetKnownHosts(listOf(r.line))
                            } catch (e: Exception) {
                                model.error = e.said()
                            }
                            rows = read()
                        },
                        deleteTitle = tr("Forget the key of {host}?", "host" to name),
                        deleteSays = tr("It is shown to check again the next time you connect."),
                        deleteLabel = tr("Forget"),
                        trailing = { r.marker?.let { Text(it, fontSize = 11.sp, color = Dusk.amber) } },
                    )
                }
            }
        }
    }
}

private fun minutes(m: Int) = when (m) {
    0 -> tr("Off")
    60 -> tr("1 hour")
    else -> trN(m, "{n} minute", "{n} minutes")
}

@Composable
private fun Row(title: String, sub: String?, subColor: Color = Dusk.muted, onClick: () -> Unit) {
    Column(
        Modifier.fillMaxWidth().heightIn(min = 56.dp).clickable(onClick = onClick)
            .padding(horizontal = 16.dp, vertical = 12.dp),
    ) {
        Text(title, fontSize = 16.sp)
        sub?.let { Text(it, fontSize = 14.sp, color = subColor) }
    }
}

@Composable
private fun SwitchRow(title: String, sub: String, on: Boolean, onChange: (Boolean) -> Unit) {
    Row(
        Modifier.fillMaxWidth().clickable { onChange(!on) }.padding(horizontal = 16.dp, vertical = 12.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Column(Modifier.weight(1f)) {
            Text(title, fontSize = 16.sp)
            Text(sub, fontSize = 14.sp, color = Dusk.muted)
        }
        Spacer(Modifier.width(16.dp))
        Switch(checked = on, onCheckedChange = onChange)
    }
}

/** A setting with a few values: its name and the value chosen; a tap opens the list. */
@Composable
private fun <T> Choice(title: String, options: List<Pair<T, String>>, value: T, onChange: (T) -> Unit) {
    var open by remember { mutableStateOf(false) }
    Box {
        Row(title, options.firstOrNull { it.first == value }?.second) { open = true }
        DropdownMenu(expanded = open, onDismissRequest = { open = false }) {
            for ((v, label) in options) {
                DropdownMenuItem(text = { Text(label) }, onClick = { open = false; onChange(v) })
            }
        }
    }
}
