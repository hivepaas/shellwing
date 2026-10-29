package com.hivepaas.shellwing.ui

import androidx.activity.compose.BackHandler
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.imePadding
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.text.KeyboardActions
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.Add
import androidx.compose.material.icons.filled.ArrowUpward
import androidx.compose.material.icons.filled.Close
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.Button
import androidx.compose.material3.DropdownMenu
import androidx.compose.material3.DropdownMenuItem
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.ExposedDropdownMenuAnchorType
import androidx.compose.material3.ExposedDropdownMenuBox
import androidx.compose.material3.ExposedDropdownMenuDefaults
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Scaffold
import androidx.compose.material3.SegmentedButton
import androidx.compose.material3.SegmentedButtonDefaults
import androidx.compose.material3.SingleChoiceSegmentedButtonRow
import androidx.compose.material3.Switch
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.material3.TopAppBar
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.text.TextStyle
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.input.ImeAction
import androidx.compose.ui.text.input.KeyboardType
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import com.hivepaas.shellwing.AppModel
import com.hivepaas.shellwing.SessionModel
import com.hivepaas.shellwing.core.AuthWay
import com.hivepaas.shellwing.core.HostEdit
import com.hivepaas.shellwing.core.HostKeyWay
import com.hivepaas.shellwing.core.KeyRow
import com.hivepaas.shellwing.core.forwardProblem
import com.hivepaas.shellwing.said
import com.hivepaas.shellwing.tr

/** The shell kept on the host: tmux's session `shellwing`, as the iOS app sets it. */
private const val TMUX = "command -v tmux >/dev/null && tmux new -A -s shellwing"

private fun blank() = HostEdit(
    id = null, name = "", alias = null, address = "", port = null, user = null, group = null,
    tags = emptyList(), jump = emptyList(), forward = emptyList(), auth = AuthWay.AUTO, key = null, theme = null,
    color = null, favorite = false, localOnly = false, forwardAgent = false, keepalive = null, hostKey = null,
    run = null,
)

/**
 * A host as the desktop's editor has it, as the iOS app's does: how it is
 * reached and signed in to, the hosts it goes through, ports forwarded, how
 * it is listed, its terminal. What the phone does not edit (the session
 * log) stays as hosts.toml has it.
 */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun HostEditorScreen(model: AppModel, hostId: String?, onDone: () -> Unit, onConnect: (SessionModel) -> Unit) {
    var edit by remember { mutableStateOf(hostId?.let { model.host(it) } ?: blank()) }
    var port by remember { mutableStateOf(edit.port?.toString() ?: "") }
    var tags by remember { mutableStateOf(edit.tags.joinToString(", ")) }
    var keepalive by remember { mutableStateOf(edit.keepalive?.toString() ?: "") }
    var forward by remember { mutableStateOf("") }
    var forwardSaid by remember { mutableStateOf<String?>(null) }
    var keys by remember { mutableStateOf(model.core?.keys() ?: emptyList()) }
    /** A key made here: the host does not have it yet. */
    var madeKey by remember { mutableStateOf<KeyRow?>(null) }
    var problem by remember { mutableStateOf<String?>(null) }
    var deleting by remember { mutableStateOf(false) }
    var picking by remember { mutableStateOf(false) }
    val themes = remember { model.core?.themes() ?: emptyList() }
    // Kept above the theme picker, which takes the screen for a moment.
    val scroll = rememberScrollState()
    val title = edit.name.ifEmpty { edit.address }
    val blankAddress = edit.address.isBlank()

    if (picking) {
        BackHandler { picking = false }
        ThemePicker(themes, edit.theme, appDefault = true, onBack = { picking = false }) {
            edit = edit.copy(theme = it)
            picking = false
        }
        return
    }

    fun save(connect: Boolean) {
        val p = port.trim()
        val number = p.toIntOrNull()
        if (p.isNotEmpty() && (number == null || number !in 1..65535)) {
            problem = tr("Port: 1 to 65535.")
            return
        }
        val k = keepalive.trim()
        if (k.isNotEmpty() && k.toULongOrNull() == null) {
            problem = tr("Keepalive: seconds, 0 to 3600 (0: none).")
            return
        }
        val done = edit.copy(
            port = number?.toUShort(),
            keepalive = k.toULongOrNull(),
            tags = tags.split(",").map { it.trim() }.filter { it.isNotEmpty() },
        )
        try {
            val id = model.save(done)
            val spec = model.sections.flatMap { it.hosts }.firstOrNull { it.id == id }?.spec
            if (connect && spec != null) model.session(spec)?.let { onConnect(it); return }
            onDone()
        } catch (e: Exception) {
            problem = e.said()
        }
    }

    fun addForward() {
        val f = forward.trim()
        if (f.isEmpty()) return
        forwardProblem(f)?.let {
            forwardSaid = it
            return
        }
        forwardSaid = null
        if (f !in edit.forward) edit = edit.copy(forward = edit.forward + f)
        forward = ""
    }

    fun makeKey() {
        try {
            val k = model.core?.generateKey(title.ifEmpty { "Android" }) ?: return
            keys = model.core?.keys() ?: emptyList()
            madeKey = k
            edit = edit.copy(key = "vault:${k.id}")
        } catch (e: Exception) {
            problem = e.said()
        }
    }

    Scaffold(
        topBar = {
            TopAppBar(
                navigationIcon = { IconButton(onClick = onDone) { Icon(Icons.Filled.Close, tr("Cancel")) } },
                title = { Text(if (hostId == null) tr("New host") else title) },
                actions = { TextButton(onClick = { save(false) }, enabled = !blankAddress) { Text(tr("Save")) } },
            )
        },
    ) { pad ->
        Column(
            Modifier.fillMaxSize().padding(pad).imePadding().verticalScroll(scroll).padding(horizontal = 16.dp),
            verticalArrangement = Arrangement.spacedBy(12.dp),
        ) {
            Section(tr("Connection"))
            Field(tr("Name"), edit.name, placeholder = edit.address.ifEmpty { "web-1" }) { edit = edit.copy(name = it) }
            Row(horizontalArrangement = Arrangement.spacedBy(12.dp)) {
                Field(tr("Address"), edit.address, Modifier.weight(2f), KeyboardType.Uri, "host.example.com", mono = true) {
                    edit = edit.copy(address = it)
                }
                Field(tr("Port"), port, Modifier.weight(1f), KeyboardType.Number, "22", mono = true) { port = it }
            }
            Field(tr("User"), edit.user ?: "", placeholder = tr("as the server says"), mono = true) {
                edit = edit.copy(user = it.ifEmpty { null })
            }
            Field(tr("Alias"), edit.alias ?: "", placeholder = tr("optional"), mono = true) {
                edit = edit.copy(alias = it.ifEmpty { null })
            }

            Section(tr("Sign in"))
            val ways = listOf(AuthWay.AUTO to tr("Automatic"), AuthWay.KEY to tr("Key"), AuthWay.PASSWORD to tr("Password")) +
                // An agent's keys are a desktop's: shown only for a host that has it.
                if (edit.auth == AuthWay.AGENT) listOf(AuthWay.AGENT to tr("Agent")) else emptyList()
            SingleChoiceSegmentedButtonRow(Modifier.fillMaxWidth()) {
                ways.forEachIndexed { i, (way, label) ->
                    SegmentedButton(
                        selected = edit.auth == way,
                        onClick = { edit = edit.copy(auth = way) },
                        shape = SegmentedButtonDefaults.itemShape(i, ways.size),
                    ) { Text(label) }
                }
            }
            if (edit.auth == AuthWay.KEY) {
                val choices = keys.map { "vault:${it.id}" to "${it.name} · ${it.kind}" } +
                    // A key file written on a desktop: kept as it is.
                    listOfNotNull(edit.key?.takeIf { it.isNotEmpty() && !it.startsWith("vault:") }?.let { it to it })
                Picker(tr("Key"), listOf<Pair<String?, String>>(null to tr("Choose a key")) + choices, edit.key) {
                    edit = edit.copy(key = it)
                }
                TextButton(onClick = { makeKey() }) {
                    Icon(Icons.Filled.Add, null)
                    Spacer(Modifier.width(8.dp))
                    Text(if (title.isEmpty()) tr("Make a key for this host") else tr("Make a key for {host}", "host" to title))
                }
                madeKey?.takeIf { edit.key == "vault:${it.id}" }?.let { k ->
                    Hint(
                        tr("{name} is new: the host does not have it yet. Connect once another way (a password), then install it from the session's menu.", "name" to k.name),
                        Dusk.amber,
                    )
                }
            }
            Hint(
                when (edit.auth) {
                    AuthWay.AUTO -> tr("Tries the keys in the vault, then asks for a password, like ssh.")
                    AuthWay.KEY ->
                        if (edit.key?.startsWith("vault:") == true) tr("Kept in the vault: no passphrase to ask for.")
                        else tr("A key file of another device: this phone signs in with the vault's keys.")
                    AuthWay.PASSWORD -> tr("Asked for when connecting, and can be saved in the vault.")
                    AuthWay.AGENT -> tr("Uses the keys in an ssh-agent: a desktop's; a phone has none.")
                },
            )

            Section(tr("Route"))
            edit.jump.forEachIndexed { i, j ->
                Row(verticalAlignment = Alignment.CenterVertically) {
                    Text("${i + 1}", fontFamily = FontFamily.Monospace, fontSize = 13.sp, color = Dusk.muted, modifier = Modifier.width(24.dp))
                    Text(j, fontFamily = FontFamily.Monospace, modifier = Modifier.weight(1f))
                    if (i > 0) {
                        IconButton(onClick = {
                            edit = edit.copy(jump = edit.jump.toMutableList().apply { add(i - 1, removeAt(i)) })
                        }) { Icon(Icons.Filled.ArrowUpward, tr("Up"), tint = Dusk.muted) }
                    }
                    IconButton(onClick = { edit = edit.copy(jump = edit.jump - j) }) {
                        Icon(Icons.Filled.Close, tr("Remove"), tint = Dusk.muted)
                    }
                }
            }
            val choices = (model.core?.jumpChoices(edit.name.ifEmpty { null }) ?: emptyList()).filter { it !in edit.jump }
            if (choices.isNotEmpty()) {
                var adding by remember { mutableStateOf(false) }
                Box {
                    TextButton(onClick = { adding = true }) {
                        Icon(Icons.Filled.Add, null)
                        Spacer(Modifier.width(8.dp))
                        Text(tr("Add a jump"))
                    }
                    DropdownMenu(expanded = adding, onDismissRequest = { adding = false }) {
                        for (c in choices) {
                            DropdownMenuItem(text = { Text(c) }, onClick = { adding = false; edit = edit.copy(jump = edit.jump + c) })
                        }
                    }
                }
            }
            Hint(
                if (edit.jump.isEmpty()) tr("Straight to the host. A jump goes through another host first (a bastion).")
                else (edit.jump + title).joinToString(" → "),
            )

            Section(tr("Port forwarding"))
            for (f in edit.forward) {
                Row(verticalAlignment = Alignment.CenterVertically) {
                    Text(f, fontFamily = FontFamily.Monospace, modifier = Modifier.weight(1f))
                    IconButton(onClick = { edit = edit.copy(forward = edit.forward - f) }) {
                        Icon(Icons.Filled.Close, tr("Remove"), tint = Dusk.muted)
                    }
                }
            }
            Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                OutlinedTextField(
                    value = forward, onValueChange = { forward = it; forwardSaid = null },
                    placeholder = { Text("-L 8080:localhost:80", fontFamily = FontFamily.Monospace) }, singleLine = true,
                    textStyle = TextStyle(fontFamily = FontFamily.Monospace),
                    keyboardOptions = KeyboardOptions(autoCorrectEnabled = false, imeAction = ImeAction.Done),
                    keyboardActions = KeyboardActions(onDone = { addForward() }),
                    isError = forwardSaid != null,
                    modifier = Modifier.weight(1f),
                )
                TextButton(onClick = { addForward() }, enabled = forward.isNotBlank()) { Text(tr("Add")) }
            }
            forwardSaid?.let { Hint(it, Dusk.bad) }
            Hint("-L [bind:]port:host:hostport · -R [bind:]port:host:hostport · -D [bind:]port\n" + tr("Set up every time this host connects."))

            Section(tr("Organize"))
            Picker(tr("Group"), listOf<Pair<String?, String>>(null to tr("None")) + model.groups().map { it to it }, edit.group) {
                edit = edit.copy(group = it)
            }
            Field(tr("Tags"), tags, placeholder = "web, docker") { tags = it }
            ColorChoice(edit.color) { edit = edit.copy(color = it) }
            SwitchLine(tr("Favorite: listed first"), null, edit.favorite) { edit = edit.copy(favorite = it) }

            Section(tr("Terminal"))
            Column(
                Modifier.fillMaxWidth().heightIn(min = 56.dp).clickable { picking = true }.padding(vertical = 8.dp),
            ) {
                Text(tr("Theme"), fontSize = 16.sp)
                Text(
                    edit.theme?.let { id -> themes.firstOrNull { it.id == id }?.name ?: id } ?: tr("Same as the app"),
                    fontSize = 14.sp, color = Dusk.muted,
                )
            }
            SwitchLine(
                tr("Keep the shell on the host (tmux)"),
                tr("In tmux, a connection made again finds the same shell, its programs still running: the phone may lose it in the background. tmux has to be on the host; without it, the shell is as usual."),
                edit.run == TMUX,
            ) { edit = edit.copy(run = if (it) TMUX else null) }
            Field(tr("Run after connecting"), edit.run ?: "", placeholder = "cd ~/apps && ls", mono = true) {
                edit = edit.copy(run = it.ifEmpty { null })
            }

            Section(tr("Advanced"))
            Picker(
                tr("Host keys"),
                listOf(
                    null to tr("As in Settings"),
                    HostKeyWay.ASK to tr("Ask"),
                    HostKeyWay.ACCEPT_NEW to tr("Accept new ones"),
                    HostKeyWay.STRICT to tr("Known ones only"),
                ),
                edit.hostKey,
            ) { edit = edit.copy(hostKey = it) }
            Field(tr("Keepalive (s)"), keepalive, type = KeyboardType.Number, placeholder = tr("as in Settings")) { keepalive = it }
            SwitchLine(
                tr("Keep on this device only"),
                if (edit.localOnly) tr("Not synced: other devices do not see this host.")
                else tr("Synced with your other devices when sync is on."),
                edit.localOnly,
            ) { edit = edit.copy(localOnly = it) }

            problem?.let { Text(it, color = Dusk.bad) }

            Row(horizontalArrangement = Arrangement.spacedBy(12.dp), modifier = Modifier.padding(top = 8.dp)) {
                OutlinedButton(onClick = { save(false) }, enabled = !blankAddress, modifier = Modifier.weight(1f)) { Text(tr("Save")) }
                Button(onClick = { save(true) }, enabled = !blankAddress, modifier = Modifier.weight(1f)) { Text(tr("Save and connect")) }
            }
            if (hostId != null) {
                TextButton(onClick = { deleting = true }) { Text(tr("Delete…"), color = Dusk.bad) }
            }
            Spacer(Modifier.height(32.dp))
        }
    }

    if (deleting && hostId != null) {
        AlertDialog(
            onDismissRequest = { deleting = false },
            title = { Text(tr("Delete {title} from hosts.toml?", "title" to title)) },
            confirmButton = {
                TextButton(onClick = { model.delete(hostId); deleting = false; onDone() }) {
                    Text(tr("Delete"), color = Dusk.bad)
                }
            },
            dismissButton = { TextButton(onClick = { deleting = false }) { Text(tr("Cancel")) } },
        )
    }
}

@Composable
private fun Section(title: String) {
    Text(title, color = Dusk.accent, fontSize = 14.sp, modifier = Modifier.padding(top = 8.dp))
}

@Composable
private fun Hint(text: String, color: Color = Dusk.muted) {
    Text(text, fontSize = 13.sp, color = color)
}

@Composable
private fun Field(
    label: String,
    value: String,
    modifier: Modifier = Modifier.fillMaxWidth(),
    type: KeyboardType = KeyboardType.Text,
    placeholder: String? = null,
    mono: Boolean = false,
    onChange: (String) -> Unit,
) {
    OutlinedTextField(
        value = value, onValueChange = onChange, label = { Text(label) }, singleLine = true,
        placeholder = placeholder?.let { { Text(it, fontFamily = if (mono) FontFamily.Monospace else null) } },
        textStyle = if (mono) TextStyle(fontFamily = FontFamily.Monospace) else TextStyle.Default,
        keyboardOptions = KeyboardOptions(keyboardType = type, autoCorrectEnabled = false),
        modifier = modifier,
    )
}

@Composable
private fun SwitchLine(title: String, sub: String?, on: Boolean, onChange: (Boolean) -> Unit) {
    Row(Modifier.fillMaxWidth().clickable { onChange(!on) }, verticalAlignment = Alignment.CenterVertically) {
        Column(Modifier.weight(1f)) {
            Text(title, fontSize = 16.sp)
            sub?.let { Text(it, fontSize = 13.sp, color = Dusk.muted) }
        }
        Spacer(Modifier.width(16.dp))
        Switch(checked = on, onCheckedChange = onChange)
    }
}

/** A field with a few values to pick from; the value shown by its label. */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
private fun <T> Picker(label: String, options: List<Pair<T, String>>, value: T, onChange: (T) -> Unit) {
    var open by remember { mutableStateOf(false) }
    ExposedDropdownMenuBox(expanded = open, onExpandedChange = { open = it }) {
        OutlinedTextField(
            value = options.firstOrNull { it.first == value }?.second ?: value.toString(), onValueChange = {},
            readOnly = true, singleLine = true, label = { Text(label) },
            trailingIcon = { ExposedDropdownMenuDefaults.TrailingIcon(open) },
            modifier = Modifier.fillMaxWidth().menuAnchor(ExposedDropdownMenuAnchorType.PrimaryNotEditable),
        )
        ExposedDropdownMenu(expanded = open, onDismissRequest = { open = false }) {
            for ((v, text) in options) DropdownMenuItem(text = { Text(text) }, onClick = { onChange(v); open = false })
        }
    }
}
