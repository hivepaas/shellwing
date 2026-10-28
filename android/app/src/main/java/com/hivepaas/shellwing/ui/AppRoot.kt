package com.hivepaas.shellwing.ui

import androidx.activity.compose.BackHandler
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.padding
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.outlined.Dns
import androidx.compose.material.icons.outlined.Lock
import androidx.compose.material.icons.outlined.Settings
import androidx.compose.material.icons.outlined.Terminal
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.Badge
import androidx.compose.material3.BadgedBox
import androidx.compose.material3.Icon
import androidx.compose.material3.NavigationBar
import androidx.compose.material3.NavigationBarItem
import androidx.compose.material3.Scaffold
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateListOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import com.hivepaas.shellwing.AppLock
import com.hivepaas.shellwing.AppModel
import com.hivepaas.shellwing.core.ConfigFileRow
import com.hivepaas.shellwing.core.EnvRow
import com.hivepaas.shellwing.core.NoteRow
import com.hivepaas.shellwing.core.SnippetEdit
import com.hivepaas.shellwing.tr

/** Where the app is: a tab, or a screen over the tabs. */
sealed interface Screen {
    data class Terminal(val sessionId: String) : Screen
    /** A host's editor; a new host's without an id. */
    data class HostEditor(val hostId: String?) : Screen
    data class SnippetEditor(val edit: SnippetEdit) : Screen
    data class EnvEditor(val env: EnvRow) : Screen
    data class NoteEditor(val note: NoteRow) : Screen
    data class ConfigFileEditor(val file: ConfigFileRow) : Screen
    data object Secrets : Screen
    data object RecoveryKey : Screen
}

enum class Tab { Hosts, Sessions, Library, Settings }

/** The app: three tabs, and the screens opened over them (a session, an editor). */
@Composable
fun AppRoot(model: AppModel, lock: AppLock, onLockNow: () -> Unit) {
    var tab by rememberSaveable { mutableStateOf(Tab.Hosts) }
    val stack = remember { mutableStateListOf<Screen>() }
    val open: (Screen) -> Unit = { stack.add(it) }
    val back: () -> Unit = { if (stack.isNotEmpty()) stack.removeAt(stack.lastIndex) }

    BackHandler(enabled = stack.isNotEmpty(), onBack = back)

    model.error?.let { said ->
        AlertDialog(
            onDismissRequest = { model.error = null },
            confirmButton = { TextButton(onClick = { model.error = null }) { Text(tr("OK")) } },
            text = { Text(said) },
        )
    }

    when (val top = stack.lastOrNull()) {
        is Screen.Terminal -> {
            val session = model.sessions.firstOrNull { it.id == top.sessionId }
            if (session == null) back() else TerminalScreen(model, session, onBack = back, onOpen = open)
            return
        }
        is Screen.HostEditor -> {
            HostEditorScreen(model, top.hostId, onDone = back, onConnect = { session ->
                back()
                open(Screen.Terminal(session.id))
            })
            return
        }
        is Screen.SnippetEditor -> { SnippetEditorScreen(model, top.edit, onDone = back); return }
        is Screen.EnvEditor -> { EnvEditorScreen(model, top.env, onDone = back); return }
        is Screen.NoteEditor -> { NoteEditorScreen(model, top.note, onDone = back); return }
        is Screen.ConfigFileEditor -> { ConfigFileEditorScreen(model, top.file, onDone = back); return }
        Screen.Secrets -> { SecretsScreen(model, onBack = back); return }
        Screen.RecoveryKey -> { RecoveryKeyScreen(model, onBack = back); return }
        null -> {}
    }

    Scaffold(
        bottomBar = {
            NavigationBar {
                NavigationBarItem(
                    selected = tab == Tab.Hosts,
                    onClick = { tab = Tab.Hosts },
                    icon = { Icon(Icons.Outlined.Dns, null) },
                    label = { Text(tr("Hosts")) },
                )
                NavigationBarItem(
                    selected = tab == Tab.Sessions,
                    onClick = { tab = Tab.Sessions },
                    icon = {
                        BadgedBox(badge = {
                            if (model.sessions.isNotEmpty()) Badge { Text("${model.sessions.size}") }
                        }) { Icon(Icons.Outlined.Terminal, null) }
                    },
                    label = { Text(tr("Sessions")) },
                )
                NavigationBarItem(
                    selected = tab == Tab.Library,
                    onClick = { tab = Tab.Library },
                    icon = { Icon(Icons.Outlined.Lock, null) },
                    label = { Text(tr("Library")) },
                )
                NavigationBarItem(
                    selected = tab == Tab.Settings,
                    onClick = { tab = Tab.Settings },
                    icon = { Icon(Icons.Outlined.Settings, null) },
                    label = { Text(tr("Settings")) },
                )
            }
        },
    ) { pad ->
        Box(Modifier.fillMaxSize().padding(pad)) {
            when (tab) {
                Tab.Hosts -> HostsScreen(model, onOpen = open, onSessions = { tab = Tab.Sessions })
                Tab.Sessions -> SessionsScreen(model, onOpen = open)
                Tab.Library -> LibraryScreen(model, onOpen = open)
                Tab.Settings -> SettingsScreen(model, lock, onLockNow = onLockNow, onOpen = open)
            }
        }
    }
}
