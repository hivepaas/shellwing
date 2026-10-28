package com.hivepaas.shellwing.ui

import android.content.ClipboardManager
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.imePadding
import androidx.compose.foundation.layout.navigationBarsPadding
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.ArrowBack
import androidx.compose.material.icons.filled.KeyboardArrowDown
import androidx.compose.material.icons.filled.KeyboardArrowLeft
import androidx.compose.material.icons.filled.KeyboardArrowRight
import androidx.compose.material.icons.filled.KeyboardArrowUp
import androidx.compose.material.icons.filled.MoreVert
import androidx.compose.material.icons.outlined.Keyboard
import androidx.compose.material.icons.outlined.KeyboardHide
import androidx.compose.material3.Button
import androidx.compose.material3.ButtonDefaults
import androidx.compose.material3.DropdownMenu
import androidx.compose.material3.DropdownMenuItem
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.Scaffold
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.material3.TopAppBar
import androidx.compose.material3.TopAppBarDefaults
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import androidx.compose.ui.viewinterop.AndroidView
import com.hivepaas.shellwing.AppModel
import com.hivepaas.shellwing.SessionModel
import com.hivepaas.shellwing.core.Key
import com.hivepaas.shellwing.core.OnDisconnectWay
import com.hivepaas.shellwing.core.SessionStatus
import com.hivepaas.shellwing.said
import com.hivepaas.shellwing.statusWord
import com.hivepaas.shellwing.tr

/** A session full screen: the terminal, why it ended or when it tries again, the keys a phone lacks. */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun TerminalScreen(model: AppModel, session: SessionModel, onBack: () -> Unit, onOpen: (Screen) -> Unit) {
    val context = LocalContext.current
    var typing by remember { mutableStateOf(true) }
    var menu by remember { mutableStateOf(false) }
    var view by remember { mutableStateOf<TerminalView?>(null) }
    val prefs = model.prefs

    // With Close as the setting, a session that ended goes, and its screen with it.
    LaunchedEffect(session.status) {
        if (session.status == SessionStatus.CLOSED && model.onDisconnect == OnDisconnectWay.CLOSE) {
            model.close(session)
            onBack()
        }
    }

    Scaffold(
        topBar = {
            TopAppBar(
                colors = TopAppBarDefaults.topAppBarColors(containerColor = Dusk.chrome),
                navigationIcon = {
                    IconButton(onClick = onBack) { Icon(Icons.AutoMirrored.Filled.ArrowBack, tr("Hosts")) }
                },
                title = {
                    Column {
                        Row(verticalAlignment = Alignment.CenterVertically) {
                            Dot(statusColor(session.status))
                            Spacer(Modifier.width(8.dp))
                            Text(session.spec, fontSize = 18.sp)
                        }
                        val sub = if (session.status == SessionStatus.CONNECTED) session.title else statusWord(session.status)
                        Text(sub, fontSize = 13.sp, color = Dusk.muted, maxLines = 1)
                    }
                },
                actions = {
                    IconButton(onClick = { menu = true }) { Icon(Icons.Filled.MoreVert, tr("Actions")) }
                    DropdownMenu(expanded = menu, onDismissRequest = { menu = false }) {
                        DropdownMenuItem(text = { Text(tr("Paste")) }, onClick = {
                            menu = false
                            val clip = context.getSystemService(ClipboardManager::class.java)?.primaryClip
                            clip?.getItemAt(0)?.coerceToText(context)?.toString()?.let { session.paste(it) }
                        })
                        DropdownMenuItem(text = { Text(tr("Back to the bottom")) }, onClick = {
                            menu = false
                            session.scrollToBottom()
                        })
                        if (!session.isLive) {
                            DropdownMenuItem(text = { Text(tr("Connect again")) }, onClick = {
                                menu = false
                                session.reconnect()
                            })
                        }
                        DropdownMenuItem(text = { Text(tr("Close")) }, onClick = {
                            menu = false
                            model.close(session)
                            onBack()
                        })
                    }
                },
            )
        },
    ) { pad ->
        Column(Modifier.fillMaxSize().padding(pad).imePadding().background(Dusk.page)) {
            AndroidView(
                factory = { ctx ->
                    TerminalView(ctx) { focused -> typing = focused }.also {
                        it.fontSize = prefs.getFloat(TerminalView.SIZE_KEY, TerminalView.DEFAULT_SIZE)
                        it.onFontSize = { size -> prefs.edit().putFloat(TerminalView.SIZE_KEY, size).apply() }
                        view = it
                        it.post { it.showKeyboard() }
                    }
                },
                update = {
                    it.model = session
                    it.frame = session.frame
                },
                modifier = Modifier.weight(1f).fillMaxWidth(),
            )

            session.notice?.let { said ->
                Bar(said, Dusk.surface, Dusk.text) { TextButton(onClick = { session.notice = null }) { Text(tr("OK")) } }
            }
            session.offer?.let { offer ->
                Bar(tr("Save {offer} in the vault? It is tried first next time.", "offer" to offer), Color(0xff3a2a14), Color(0xfffde68a)) {
                    TextButton(onClick = { session.dismissOffer() }) { Text(tr("Not now"), color = Color(0xfffde68a)) }
                    Button(
                        onClick = {
                            runCatching { session.keepOffer() }.onFailure {
                                session.notice = tr("Not saved: {error}", "error" to it.said())
                            }
                        },
                        colors = ButtonDefaults.buttonColors(containerColor = Dusk.amber, contentColor = Color(0xff241703)),
                    ) { Text(tr("Save")) }
                }
            }
            val retrying = session.retrying
            if (retrying != null) {
                Bar(
                    tr(
                        "Connection lost. Trying again in {seconds} s (attempt {attempt} of {attempts}).",
                        "seconds" to "${retrying.seconds}", "attempt" to "${retrying.attempt}",
                        "attempts" to "${retrying.attempts}",
                    ),
                    Dusk.surface, Color(0xfffdba74),
                ) {
                    TextButton(onClick = { session.stopRetrying() }) { Text(tr("Stop")) }
                    TextButton(onClick = { session.reconnect() }) { Text(tr("Connect again")) }
                }
            } else if (!session.isLive) {
                Bar(session.ended ?: statusWord(session.status), Dusk.surface, Dusk.soft) {
                    TextButton(onClick = { session.reconnect() }) { Text(tr("Connect again")) }
                }
            }

            KeysRow(session, typing = typing, onKeyboard = {
                val v = view ?: return@KeysRow
                if (typing) v.hideKeyboard() else v.showKeyboard()
            })
        }
    }
}

@Composable
private fun Bar(text: String, background: Color, color: Color, actions: @Composable () -> Unit) {
    Row(
        Modifier.fillMaxWidth().background(background).padding(start = 14.dp, end = 6.dp, top = 6.dp, bottom = 6.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Text(text, color = color, fontSize = 14.sp, modifier = Modifier.weight(1f))
        actions()
    }
}

/** The keys a phone's keyboard lacks: Esc, Tab, Ctrl and Alt (held for the next key), the arrows, a few symbols. */
@Composable
private fun KeysRow(session: SessionModel, typing: Boolean, onKeyboard: () -> Unit) {
    Column(
        Modifier.fillMaxWidth().background(Dusk.chrome).padding(horizontal = 6.dp, vertical = 8.dp)
            .navigationBarsPadding(),
        verticalArrangement = Arrangement.spacedBy(5.dp),
    ) {
        Row(horizontalArrangement = Arrangement.spacedBy(5.dp)) {
            KeyButton("Esc") { session.press(Key.Escape) }
            KeyButton("Tab") { session.press(Key.Tab) }
            KeyButton("Ctrl", on = session.ctrl) { session.ctrl = !session.ctrl }
            KeyButton("Alt", on = session.alt) { session.alt = !session.alt }
            KeyButton(icon = Icons.Filled.KeyboardArrowLeft, label = tr("Left")) { session.press(Key.Left) }
            KeyButton(icon = Icons.Filled.KeyboardArrowUp, label = tr("Up")) { session.press(Key.Up) }
            KeyButton(icon = Icons.Filled.KeyboardArrowDown, label = tr("Down")) { session.press(Key.Down) }
            KeyButton(icon = Icons.Filled.KeyboardArrowRight, label = tr("Right")) { session.press(Key.Right) }
        }
        Row(horizontalArrangement = Arrangement.spacedBy(5.dp)) {
            for (c in listOf("|", "~", "/", "-", "_", ":")) KeyButton(c, mono = true) { session.type(c) }
            KeyButton(
                icon = if (typing) Icons.Outlined.KeyboardHide else Icons.Outlined.Keyboard,
                label = if (typing) tr("Hide the keyboard") else tr("Show the keyboard"),
                accent = true,
                onClick = onKeyboard,
            )
        }
    }
}

@Composable
private fun androidx.compose.foundation.layout.RowScope.KeyButton(
    text: String? = null,
    icon: ImageVector? = null,
    label: String? = null,
    on: Boolean = false,
    mono: Boolean = false,
    accent: Boolean = false,
    onClick: () -> Unit,
) {
    val shape = RoundedCornerShape(10.dp)
    val bg = when {
        accent -> Dusk.accent
        on -> Dusk.accentContainer
        else -> Dusk.button
    }
    val fg = when {
        accent -> Dusk.onAccent
        on -> Dusk.onAccentContainer
        else -> Dusk.text
    }
    TextButton(
        onClick = onClick,
        shape = shape,
        contentPadding = androidx.compose.foundation.layout.PaddingValues(0.dp),
        colors = ButtonDefaults.textButtonColors(containerColor = bg, contentColor = fg),
        modifier = Modifier.weight(1f).height(44.dp).clip(shape)
            .then(if (on) Modifier.border(1.dp, Dusk.accent, shape) else Modifier),
    ) {
        if (icon != null) {
            Icon(icon, label)
        } else {
            Text(
                text ?: "",
                fontSize = if (mono) 15.sp else 14.sp,
                fontWeight = FontWeight.Medium,
                fontFamily = if (mono) FontFamily.Monospace else FontFamily.Default,
            )
        }
    }
}
