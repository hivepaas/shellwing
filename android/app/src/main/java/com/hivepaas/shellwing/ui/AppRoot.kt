package com.hivepaas.shellwing.ui

import androidx.activity.compose.BackHandler
import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxHeight
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.statusBarsPadding
import androidx.compose.foundation.layout.systemBarsPadding
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.layout.WindowInsets
import androidx.compose.foundation.layout.WindowInsetsSides
import androidx.compose.foundation.layout.only
import androidx.compose.foundation.layout.safeDrawing
import androidx.compose.foundation.layout.windowInsetsPadding
import androidx.compose.material.icons.filled.Close
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.NavigationRail
import androidx.compose.material3.NavigationRailItem
import androidx.compose.material3.PrimaryScrollableTabRow
import androidx.compose.material3.Tab
import androidx.compose.material3.VerticalDivider
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.key
import androidx.compose.ui.Alignment
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.platform.LocalConfiguration
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
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
import androidx.compose.runtime.saveable.rememberSaveableStateHolder
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
    /** A session's files over SFTP. */
    data class Files(val sessionId: String) : Screen
    /** A host's editor; a new host's without an id. */
    data class HostEditor(val hostId: String?) : Screen
    /** A group's editor; a new group's without a name. */
    data class GroupEditor(val name: String?) : Screen
    data class SnippetEditor(val edit: SnippetEdit) : Screen
    data class EnvEditor(val env: EnvRow) : Screen
    data class NoteEditor(val note: NoteRow) : Screen
    data class ConfigFileEditor(val file: ConfigFileRow) : Screen
    data object Secrets : Screen
    data object RecoveryKey : Screen
    data object SnippetLibrary : Screen
    data object Sync : Screen
}

enum class Tab { Hosts, Sessions, Library, Settings }

/** A tablet's width: the rail in place of the bottom bar. */
private const val RAIL_DP = 600

/** Wide enough for the host list beside a session, as the design's tablet has it. */
private const val SPLIT_DP = 840

/** And tall enough: a phone on its side is wide, but its keyboard leaves the session little room. */
private const val SPLIT_HEIGHT_DP = 480

/**
 * The app: four tabs, and the screens opened over them (a session, an
 * editor). On a phone the tabs are at the bottom and a session fills the
 * screen; on a tablet they are a rail, and from [SPLIT_DP] the hosts and
 * the sessions list sit beside the session open, its tabs above it.
 */
@Composable
fun AppRoot(model: AppModel, lock: AppLock, onLockNow: () -> Unit) {
    var tab by rememberSaveable { mutableStateOf(Tab.Hosts) }
    val stack = remember { mutableStateListOf<Screen>() }
    // Each tab keeps what it was showing (Library's Notes, a filter) while a
    // screen is over it, or another tab is.
    val kept = rememberSaveableStateHolder()
    val screen = LocalConfiguration.current
    val width = screen.screenWidthDp
    val split = width >= SPLIT_DP && screen.screenHeightDp >= SPLIT_HEIGHT_DP
    /** The session beside the list, when split. */
    var current by rememberSaveable { mutableStateOf<String?>(null) }
    val open: (Screen) -> Unit = { screen ->
        if (split && screen is Screen.Terminal) {
            current = screen.sessionId
            if (tab != Tab.Hosts && tab != Tab.Sessions) tab = Tab.Sessions
        } else {
            stack.add(screen)
        }
    }
    val back: () -> Unit = { if (stack.isNotEmpty()) stack.removeAt(stack.lastIndex) }

    // Turned to a width that splits: a session on top goes beside the list.
    LaunchedEffect(split, stack.lastOrNull()) {
        val top = stack.lastOrNull()
        if (split && top is Screen.Terminal) {
            stack.removeAt(stack.lastIndex)
            current = top.sessionId
        }
    }

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
        is Screen.Files -> {
            val session = model.sessions.firstOrNull { it.id == top.sessionId }
            if (session == null) back() else FilesScreen(model, session, onBack = back)
            return
        }
        is Screen.HostEditor -> {
            HostEditorScreen(model, top.hostId, onDone = back, onConnect = { session ->
                back()
                open(Screen.Terminal(session.id))
            })
            return
        }
        is Screen.GroupEditor -> { GroupEditorScreen(model, top.name, onDone = back); return }
        is Screen.SnippetEditor -> { SnippetEditorScreen(model, top.edit, onDone = back); return }
        is Screen.EnvEditor -> { EnvEditorScreen(model, top.env, onDone = back); return }
        is Screen.NoteEditor -> { NoteEditorScreen(model, top.note, onDone = back); return }
        is Screen.ConfigFileEditor -> { ConfigFileEditorScreen(model, top.file, onDone = back); return }
        Screen.Secrets -> { SecretsScreen(model, onBack = back); return }
        Screen.RecoveryKey -> { RecoveryKeyScreen(model, onBack = back); return }
        Screen.SnippetLibrary -> { SnippetLibraryScreen(model, onBack = back); return }
        Screen.Sync -> { SyncScreen(model, onBack = back); return }
        null -> {}
    }

    val tabs = listOf(
        Triple(Tab.Hosts, tr("Hosts"), Icons.Outlined.Dns),
        Triple(Tab.Sessions, tr("Sessions"), Icons.Outlined.Terminal),
        Triple(Tab.Library, tr("Library"), Icons.Outlined.Lock),
        Triple(Tab.Settings, tr("Settings"), Icons.Outlined.Settings),
    )
    val icon: @Composable (Tab, ImageVector) -> Unit = { t, image ->
        if (t == Tab.Sessions) {
            BadgedBox(badge = { if (model.sessions.isNotEmpty()) Badge { Text("${model.sessions.size}") } }) { Icon(image, null) }
        } else {
            Icon(image, null)
        }
    }
    val content: @Composable () -> Unit = {
        kept.SaveableStateProvider(tab.name) {
            when (tab) {
                Tab.Hosts -> HostsScreen(model, onOpen = open, onSessions = { tab = Tab.Sessions })
                Tab.Sessions -> SessionsScreen(model, onOpen = open)
                Tab.Library -> LibraryScreen(model, onOpen = open)
                Tab.Settings -> SettingsScreen(model, lock, onLockNow = onLockNow, onOpen = open)
            }
        }
    }

    if (width < RAIL_DP) {
        Scaffold(
            bottomBar = {
                NavigationBar {
                    for ((t, label, image) in tabs) {
                        NavigationBarItem(selected = tab == t, onClick = { tab = t }, icon = { icon(t, image) }, label = { Text(label) })
                    }
                }
            },
        ) { pad -> Box(Modifier.fillMaxSize().padding(pad)) { content() } }
        return
    }

    // The sides' insets once, for the row: the panes in it are not at the window's edges.
    Row(Modifier.fillMaxSize().background(Dusk.panel).windowInsetsPadding(WindowInsets.safeDrawing.only(WindowInsetsSides.Horizontal))) {
        NavigationRail(containerColor = Dusk.chrome, modifier = Modifier.systemBarsPadding()) {
            Spacer(Modifier.height(12.dp))
            for ((t, label, image) in tabs) {
                NavigationRailItem(selected = tab == t, onClick = { tab = t }, icon = { icon(t, image) }, label = { Text(label) })
            }
        }
        if (split && (tab == Tab.Hosts || tab == Tab.Sessions)) {
            Box(Modifier.width(340.dp).fillMaxHeight()) { content() }
            VerticalDivider(color = Dusk.line)
            SessionPane(model, current, onCurrent = { current = it }, onOpen = open, modifier = Modifier.weight(1f))
        } else {
            Box(Modifier.weight(1f).fillMaxHeight()) { content() }
        }
    }
}

/**
 * The sessions beside the list on a wide screen: a tab for each, the one
 * chosen below them (the first when none is).
 */
@Composable
private fun SessionPane(
    model: AppModel,
    current: String?,
    onCurrent: (String?) -> Unit,
    onOpen: (Screen) -> Unit,
    modifier: Modifier,
) {
    val sessions = model.sessions
    val shown = sessions.firstOrNull { it.id == current } ?: sessions.firstOrNull()
    Column(modifier.fillMaxHeight().background(Dusk.page)) {
        if (shown == null) {
            Column(
                Modifier.fillMaxSize().padding(32.dp),
                horizontalAlignment = Alignment.CenterHorizontally,
                verticalArrangement = Arrangement.Center,
            ) {
                Text(tr("No sessions open"), style = MaterialTheme.typography.titleMedium, color = Dusk.text)
                Spacer(Modifier.height(8.dp))
                Text(tr("Open one from Hosts."), color = Dusk.muted)
            }
            return@Column
        }
        PrimaryScrollableTabRow(
            selectedTabIndex = sessions.indexOf(shown), edgePadding = 8.dp, containerColor = Dusk.chrome,
            modifier = Modifier.statusBarsPadding(),
        ) {
            for (s in sessions) {
                Tab(selected = s == shown, onClick = { onCurrent(s.id) }) {
                    Row(Modifier.padding(start = 14.dp, top = 6.dp, bottom = 6.dp), verticalAlignment = Alignment.CenterVertically) {
                        Dot(statusColor(s.status))
                        Spacer(Modifier.width(8.dp))
                        Text(s.spec, fontSize = 14.sp, maxLines = 1)
                        IconButton(onClick = {
                            model.close(s)
                            if (s == shown) onCurrent(null)
                        }) { Icon(Icons.Filled.Close, tr("Close"), modifier = Modifier.size(18.dp), tint = Dusk.muted) }
                    }
                }
            }
        }
        // Its own state and terminal view for each session.
        key(shown.id) {
            TerminalScreen(model, shown, onBack = { onCurrent(null) }, onOpen = onOpen, embedded = true)
        }
    }
}
