package com.hivepaas.shellwing.ui

import android.content.ClipData
import android.content.ClipboardManager
import android.content.Context
import android.content.Intent
import android.net.Uri
import android.provider.DocumentsContract
import android.provider.OpenableColumns
import android.text.format.DateUtils
import android.text.format.Formatter
import android.webkit.MimeTypeMap
import androidx.activity.compose.BackHandler
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.result.contract.ActivityResultContract
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.foundation.ExperimentalFoundationApi
import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.combinedClickable
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.imePadding
import androidx.compose.foundation.layout.navigationBarsPadding
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.text.BasicTextField
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.ArrowBack
import androidx.compose.material.icons.automirrored.filled.KeyboardArrowRight
import androidx.compose.material.icons.filled.Add
import androidx.compose.material.icons.filled.ArrowDropDown
import androidx.compose.material.icons.filled.Check
import androidx.compose.material.icons.filled.Close
import androidx.compose.material.icons.filled.Folder
import androidx.compose.material.icons.outlined.Description
import androidx.compose.material.icons.outlined.Link
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.DropdownMenu
import androidx.compose.material3.DropdownMenuItem
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.Scaffold
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.material3.TopAppBar
import androidx.compose.material3.pulltorefresh.PullToRefreshBox
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateListOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.SolidColor
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.text.TextStyle
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.input.KeyboardCapitalization
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import androidx.core.content.FileProvider
import com.hivepaas.shellwing.AppModel
import com.hivepaas.shellwing.SessionModel
import com.hivepaas.shellwing.core.FileEntry
import com.hivepaas.shellwing.core.MobileSession
import com.hivepaas.shellwing.core.SessionStatus
import com.hivepaas.shellwing.said
import com.hivepaas.shellwing.tr
import com.hivepaas.shellwing.trN
import java.io.File
import java.nio.ByteBuffer
import java.nio.charset.CodingErrorAction
import java.util.UUID
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.delay
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext

/** Larger than this (1 MiB) is not a file to edit on a phone. */
private const val MOST_TO_EDIT: ULong = 1048576u

/** A remote text file being edited: where it is, its copy here, and what the server had when it was opened. */
private class RemoteText(val remote: String, val local: File, val text: String, val size: ULong, val modified: ULong?)

/** A name asked for: a new folder, or an entry renamed. */
private class Naming(val renaming: FileEntry?, val text: String)

/** A file to save where the picker says: its name, its path on the host, and its copy here if made already. */
private class Out(val name: String, val remote: String, val copy: File?)

/**
 * A session's files over SFTP, as the iOS app's FilesScreen and the
 * desktop's Files panel: a folder at a time, opened, made, renamed, removed;
 * text files edited; files saved, shared or opened in another app. Android's
 * pickers hand content URIs and the core reads and writes paths, so what goes
 * either way is copied through the app's cache.
 */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun FilesScreen(model: AppModel, session: SessionModel, onBack: () -> Unit) {
    val context = LocalContext.current
    val scope = rememberCoroutineScope()
    var dir by remember { mutableStateOf("") }
    var entries by remember { mutableStateOf<List<FileEntry>>(emptyList()) }
    var loading by remember { mutableStateOf(false) }
    var busy by remember { mutableStateOf<String?>(null) }
    var problem by remember { mutableStateOf<String?>(null) }
    var said by remember { mutableStateOf<String?>(null) }
    var naming by remember { mutableStateOf<Naming?>(null) }
    var removing by remember { mutableStateOf<FileEntry?>(null) }
    var showHidden by remember { mutableStateOf(false) }
    var editing by remember { mutableStateOf<RemoteText?>(null) }
    var adding by remember { mutableStateOf(false) }
    var pathMenu by remember { mutableStateOf(false) }
    // The folders come from, for Back.
    val trail = remember { mutableStateListOf<String>() }
    var saving by remember { mutableStateOf<Out?>(null) }
    var savingFolder by remember { mutableStateOf<Out?>(null) }

    fun handle(): MobileSession {
        val h = session.handle
        if (h == null || session.status != SessionStatus.CONNECTED) error(tr("Not connected."))
        return h
    }

    suspend fun list(path: String): Boolean {
        loading = true
        try {
            entries = handle().filesList(path)
            dir = path
            problem = null
            return true
        } catch (e: CancellationException) {
            throw e
        } catch (e: Exception) {
            problem = e.said()
            return false
        } finally {
            loading = false
        }
    }

    fun go(path: String) {
        val from = dir
        scope.launch { if (list(path) && from.isNotEmpty() && from != path) trail.add(from) }
    }

    fun show(text: String) {
        said = text
        scope.launch {
            delay(4000)
            if (said == text) said = null
        }
    }

    /**
     * Does [work] with [doing] said; then says what it returns, or why it
     * failed. On the app's scope: a transfer goes on when the screen is left.
     */
    fun run(doing: String, reload: Boolean = true, work: suspend () -> String?) {
        busy = doing
        model.scope.launch {
            try {
                val done = work()
                busy = null
                done?.let(::show)
            } catch (e: CancellationException) {
                throw e
            } catch (e: Exception) {
                busy = null
                show(e.said())
            }
            if (reload) list(dir)
        }
    }

    val saver = rememberLauncherForActivityResult(SaveAs) { uri ->
        val out = saving ?: return@rememberLauncherForActivityResult
        saving = null
        if (uri == null) {
            out.copy?.parentFile?.deleteRecursively()
            return@rememberLauncherForActivityResult
        }
        run(tr("Downloading {name}…", "name" to out.name), reload = false) {
            val local = out.copy ?: fresh(context, out.name).also { handle().filesDownload(out.remote, it.path) }
            withContext(Dispatchers.IO) {
                try {
                    context.contentResolver.openOutputStream(uri)?.use { o -> local.inputStream().use { it.copyTo(o) } }
                } finally {
                    local.parentFile?.deleteRecursively()
                }
            }
            tr("Saved")
        }
    }
    val folderSaver = rememberLauncherForActivityResult(ActivityResultContracts.OpenDocumentTree()) { tree ->
        val out = savingFolder ?: return@rememberLauncherForActivityResult
        savingFolder = null
        tree ?: return@rememberLauncherForActivityResult
        run(tr("Downloading {name}…", "name" to out.name), reload = false) {
            val local = fresh(context, out.name)
            handle().filesDownload(out.remote, local.path)
            withContext(Dispatchers.IO) {
                try {
                    copyInto(context, local, DocumentsContract.buildDocumentUriUsingTree(tree, DocumentsContract.getTreeDocumentId(tree)))
                } finally {
                    local.parentFile?.deleteRecursively()
                }
            }
            tr("Saved")
        }
    }
    val picker = rememberLauncherForActivityResult(ActivityResultContracts.OpenMultipleDocuments()) { uris ->
        if (uris.isEmpty()) return@rememberLauncherForActivityResult
        val names = uris.map { displayName(context, it) }
        val at = dir
        run(if (uris.size == 1) tr("Uploading {name}…", "name" to names[0]) else trN(uris.size, "Uploading {n} item…", "Uploading {n} items…")) {
            for ((uri, name) in uris.zip(names)) {
                val local = fresh(context, name)
                try {
                    withContext(Dispatchers.IO) {
                        context.contentResolver.openInputStream(uri)?.use { i -> local.outputStream().use { i.copyTo(it) } }
                    }
                    handle().filesUpload(local.path, join(at, name))
                } finally {
                    local.parentFile?.deleteRecursively()
                }
            }
            if (uris.size == 1) tr("{name} uploaded.", "name" to names[0]) else trN(uris.size, "{n} item uploaded.", "{n} items uploaded.")
        }
    }

    LaunchedEffect(Unit) {
        withContext(Dispatchers.IO) { tidy(context) }
        try {
            list(handle().filesHome())
        } catch (e: CancellationException) {
            throw e
        } catch (e: Exception) {
            problem = e.said()
        }
    }

    fun download(e: FileEntry) {
        val out = Out(e.name, join(dir, e.name), null)
        if (e.dir) {
            savingFolder = out
            folderSaver.launch(null)
        } else {
            saving = out
            saver.launch(e.name)
        }
    }

    /** Downloads [e] to the cache, then hands it to another app: to show it, or to send it. */
    fun handOut(e: FileEntry, view: Boolean) {
        val remote = join(dir, e.name)
        run(tr("Opening {name}…", "name" to e.name), reload = false) {
            val local = fresh(context, e.name)
            handle().filesDownload(remote, local.path)
            showOrSend(context, local, view)
            null
        }
    }

    /** Opens a text file to edit; one that is not text is shown by another app, or saved when no type says which. */
    fun edit(e: FileEntry) {
        val remote = join(dir, e.name)
        run(tr("Opening {name}…", "name" to e.name), reload = false) {
            val local = fresh(context, e.name)
            handle().filesDownload(remote, local.path)
            val text = withContext(Dispatchers.IO) { textOf(local.readBytes()) }
            when {
                text != null -> editing = RemoteText(remote, local, text, e.size, e.modified)
                known(e.name) -> showOrSend(context, local, view = true)
                else -> {
                    saving = Out(e.name, remote, local)
                    saver.launch(e.name)
                }
            }
            null
        }
    }

    fun keep(e: FileEntry) {
        val core = model.core ?: return
        val h = session.handle ?: return
        val path = join(dir, e.name)
        run(tr("Keeping {name}…", "name" to e.name), reload = false) {
            core.keepHostFile(h, path)
            tr("{name} is in the vault: Library › Files.", "name" to e.name)
        }
    }

    fun named(n: Naming) {
        naming = null
        val text = n.text.trim()
        if (text.isEmpty() || text.contains('/')) {
            show(tr("A name without /, please."))
            return
        }
        val at = dir
        run(if (n.renaming == null) tr("Making {name}…", "name" to text) else tr("Renaming…")) {
            val old = n.renaming
            if (old != null) handle().filesRename(join(at, old.name), join(at, text)) else handle().filesMkdir(join(at, text))
            null
        }
    }

    fun remove(e: FileEntry) {
        removing = null
        val path = join(dir, e.name)
        run(tr("Deleting {name}…", "name" to e.name)) {
            handle().filesRemove(path)
            tr("{name} deleted.", "name" to e.name)
        }
    }

    val open = editing
    if (open != null) {
        RemoteTextEditor(session, open) {
            editing = null
            open.local.parentFile?.deleteRecursively()
            scope.launch { list(dir) }
        }
        return
    }

    BackHandler(enabled = trail.isNotEmpty()) {
        val to = trail.removeAt(trail.lastIndex)
        scope.launch { list(to) }
    }

    val shown = if (showHidden) entries else entries.filter { !it.name.startsWith(".") }
    Scaffold(
        topBar = {
            TopAppBar(
                navigationIcon = { IconButton(onClick = onBack) { Icon(Icons.AutoMirrored.Filled.ArrowBack, tr("Done")) } },
                title = {
                    Box {
                        val up = dir.isNotEmpty() && dir != "/"
                        Column(Modifier.clickable(enabled = up) { pathMenu = true }) {
                            Row(verticalAlignment = Alignment.CenterVertically) {
                                Text(name(dir), fontSize = 18.sp, maxLines = 1, overflow = TextOverflow.Ellipsis, modifier = Modifier.weight(1f, fill = false))
                                if (up) Icon(Icons.Filled.ArrowDropDown, null, tint = Dusk.muted)
                            }
                            Text(session.spec, fontSize = 13.sp, color = Dusk.muted, maxLines = 1)
                        }
                        DropdownMenu(expanded = pathMenu, onDismissRequest = { pathMenu = false }) {
                            for (p in parents(dir)) {
                                DropdownMenuItem(text = { Text(p, fontFamily = FontFamily.Monospace, fontSize = 14.sp) }, onClick = { pathMenu = false; go(p) })
                            }
                        }
                    }
                },
                actions = {
                    Box {
                        IconButton(onClick = { adding = true }, enabled = busy == null && dir.isNotEmpty()) { Icon(Icons.Filled.Add, tr("Add")) }
                        DropdownMenu(expanded = adding, onDismissRequest = { adding = false }) {
                            DropdownMenuItem(text = { Text(tr("Upload from Files…")) }, onClick = { adding = false; picker.launch(arrayOf("*/*")) })
                            DropdownMenuItem(text = { Text(tr("New folder")) }, onClick = { adding = false; naming = Naming(null, "") })
                            DropdownMenuItem(
                                text = { Text(tr("Hidden files")) },
                                trailingIcon = { if (showHidden) Icon(Icons.Filled.Check, null) },
                                onClick = { adding = false; showHidden = !showHidden },
                            )
                            DropdownMenuItem(text = { Text(tr("Refresh")) }, onClick = { adding = false; scope.launch { list(dir) } })
                        }
                    }
                },
            )
        },
        bottomBar = {
            val text = busy ?: said
            if (text != null) {
                Row(
                    Modifier.fillMaxWidth().background(Dusk.raised).navigationBarsPadding().padding(12.dp),
                    verticalAlignment = Alignment.CenterVertically,
                ) {
                    if (busy != null) {
                        CircularProgressIndicator(Modifier.size(18.dp), strokeWidth = 2.dp)
                        Spacer(Modifier.width(10.dp))
                    }
                    Text(text, fontSize = 13.sp)
                }
            }
        },
    ) { pad ->
        PullToRefreshBox(
            isRefreshing = loading && entries.isNotEmpty(),
            onRefresh = { scope.launch { list(dir) } },
            modifier = Modifier.fillMaxSize().padding(pad),
        ) {
            LazyColumn(Modifier.fillMaxSize(), contentPadding = PaddingValues(bottom = 24.dp)) {
                problem?.let { item { Text(it, color = Dusk.bad, modifier = Modifier.padding(16.dp)) } }
                if (!loading && entries.isEmpty() && problem == null) {
                    item { Text(tr("Nothing here."), color = Dusk.muted, modifier = Modifier.padding(16.dp)) }
                }
                items(shown, key = { it.name }) { e ->
                    FileRow(
                        e, detail(context, e), enabled = busy == null,
                        onClick = {
                            when {
                                e.dir -> go(join(dir, e.name))
                                e.size <= MOST_TO_EDIT -> edit(e)
                                known(e.name) -> handOut(e, view = true)
                                else -> download(e)
                            }
                        },
                    ) { close ->
                        if (!e.dir && e.size <= MOST_TO_EDIT) {
                            DropdownMenuItem(text = { Text(tr("Edit")) }, onClick = { close(); edit(e) })
                        }
                        if (!e.dir) {
                            DropdownMenuItem(text = { Text(tr("Open with default app")) }, onClick = { close(); handOut(e, view = true) })
                            DropdownMenuItem(text = { Text(tr("Share")) }, onClick = { close(); handOut(e, view = false) })
                        }
                        DropdownMenuItem(
                            text = { Text(if (e.dir) tr("Download the folder") else tr("Download")) },
                            onClick = { close(); download(e) },
                        )
                        if (!e.dir) {
                            DropdownMenuItem(text = { Text(tr("Keep in the vault")) }, onClick = { close(); keep(e) })
                        }
                        DropdownMenuItem(text = { Text(tr("Rename")) }, onClick = { close(); naming = Naming(e, e.name) })
                        DropdownMenuItem(text = { Text(tr("Copy the path")) }, onClick = {
                            close()
                            context.getSystemService(ClipboardManager::class.java)
                                ?.setPrimaryClip(ClipData.newPlainText(e.name, join(dir, e.name)))
                        })
                        DropdownMenuItem(text = { Text(tr("Delete"), color = Dusk.bad) }, onClick = { close(); removing = e })
                    }
                }
            }
        }
    }

    naming?.let { n ->
        var text by remember(n) { mutableStateOf(n.text) }
        AlertDialog(
            onDismissRequest = { naming = null },
            title = { Text(if (n.renaming == null) tr("New folder") else tr("Rename")) },
            text = { PlainField(tr("Name"), text) { text = it } },
            confirmButton = {
                TextButton(onClick = { named(Naming(n.renaming, text)) }) { Text(if (n.renaming == null) tr("Make") else tr("Rename")) }
            },
            dismissButton = { TextButton(onClick = { naming = null }) { Text(tr("Cancel")) } },
        )
    }
    removing?.let { e ->
        AlertDialog(
            onDismissRequest = { removing = null },
            title = { Text(tr("Delete {name}?", "name" to e.name)) },
            text = {
                Text(
                    if (e.dir) tr("The folder and everything in it are deleted on {host}.", "host" to session.spec)
                    else tr("It is deleted on {host}.", "host" to session.spec),
                )
            },
            confirmButton = { TextButton(onClick = { remove(e) }) { Text(tr("Delete"), color = Dusk.bad) } },
            dismissButton = { TextButton(onClick = { removing = null }) { Text(tr("Cancel")) } },
        )
    }
}

/** An entry: a tap opens it, a long press offers [menu]. */
@OptIn(ExperimentalFoundationApi::class)
@Composable
private fun FileRow(
    e: FileEntry,
    detail: String,
    enabled: Boolean,
    onClick: () -> Unit,
    menu: @Composable (close: () -> Unit) -> Unit,
) {
    var open by remember { mutableStateOf(false) }
    Box {
        Row(
            Modifier.fillMaxWidth()
                .combinedClickable(enabled = enabled, onClick = onClick, onLongClick = { open = true })
                .padding(horizontal = 16.dp, vertical = 10.dp),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            Icon(
                when {
                    e.dir -> Icons.Filled.Folder
                    e.link -> Icons.Outlined.Link
                    else -> Icons.Outlined.Description
                },
                null, tint = if (e.dir) Dusk.accent else Dusk.soft, modifier = Modifier.size(24.dp),
            )
            Spacer(Modifier.width(14.dp))
            Column(Modifier.weight(1f)) {
                Text(e.name, fontSize = 16.sp, maxLines = 1, overflow = TextOverflow.Ellipsis)
                if (detail.isNotEmpty()) Text(detail, fontSize = 12.sp, color = Dusk.muted)
            }
            if (e.dir) Icon(Icons.AutoMirrored.Filled.KeyboardArrowRight, null, tint = Dusk.muted)
        }
        DropdownMenu(expanded = open, onDismissRequest = { open = false }) { menu { open = false } }
    }
}

/**
 * A remote file's text, saved back over SFTP; if the server's copy changed
 * since it was opened, asked before it is replaced.
 */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
private fun RemoteTextEditor(session: SessionModel, file: RemoteText, onDone: () -> Unit) {
    val scope = rememberCoroutineScope()
    var text by remember { mutableStateOf(file.text) }
    var saving by remember { mutableStateOf(false) }
    var problem by remember { mutableStateOf<String?>(null) }
    var changedThere by remember { mutableStateOf(false) }
    BackHandler(onBack = onDone)

    fun save(force: Boolean) {
        val h = session.handle ?: return
        saving = true
        scope.launch {
            try {
                if (!force) {
                    // What the server has now, against what it had.
                    val cut = file.remote.lastIndexOf('/')
                    val parent = if (cut <= 0) "/" else file.remote.substring(0, cut)
                    val now = h.filesList(parent).firstOrNull { it.name == file.remote.substring(cut + 1) }
                    if (now != null && (now.size != file.size || now.modified != file.modified)) {
                        changedThere = true
                        saving = false
                        return@launch
                    }
                }
                withContext(Dispatchers.IO) { file.local.writeText(text) }
                h.filesUpload(file.local.path, file.remote)
                onDone()
            } catch (e: CancellationException) {
                throw e
            } catch (e: Exception) {
                problem = tr("Not saved: {error}", "error" to e.said())
            }
            saving = false
        }
    }

    Scaffold(
        topBar = {
            TopAppBar(
                navigationIcon = { IconButton(onClick = onDone) { Icon(Icons.Filled.Close, tr("Cancel")) } },
                title = {
                    Column {
                        Text(file.remote.substringAfterLast('/'), fontSize = 18.sp, maxLines = 1, overflow = TextOverflow.Ellipsis)
                        Text(session.spec, fontSize = 13.sp, color = Dusk.muted, maxLines = 1)
                    }
                },
                actions = {
                    if (saving) CircularProgressIndicator(Modifier.size(20.dp), strokeWidth = 2.dp)
                    TextButton(onClick = { save(false) }, enabled = text != file.text && !saving) { Text(tr("Save")) }
                },
            )
        },
    ) { pad ->
        Column(Modifier.fillMaxSize().padding(pad).imePadding()) {
            problem?.let {
                Text(it, color = Dusk.bad, fontSize = 13.sp, modifier = Modifier.fillMaxWidth().background(Dusk.surface).padding(10.dp))
            }
            BasicTextField(
                value = text, onValueChange = { text = it },
                textStyle = TextStyle(fontFamily = FontFamily.Monospace, fontSize = 13.sp, color = Dusk.text),
                cursorBrush = SolidColor(Dusk.accent),
                keyboardOptions = KeyboardOptions(capitalization = KeyboardCapitalization.None, autoCorrectEnabled = false),
                modifier = Modifier.fillMaxSize().background(Dusk.field).padding(10.dp),
            )
        }
    }
    if (changedThere) {
        AlertDialog(
            onDismissRequest = { changedThere = false },
            text = { Text(tr("{path} changed on {host} since it was opened.", "path" to file.remote, "host" to session.spec)) },
            confirmButton = {
                TextButton(onClick = { changedThere = false; save(true) }) { Text(tr("Replace it with this"), color = Dusk.bad) }
            },
            dismissButton = { TextButton(onClick = { changedThere = false }) { Text(tr("Cancel")) } },
        )
    }
}

/** The picker that asks where to save a file, with the file's name and its type. */
private object SaveAs : ActivityResultContract<String, Uri?>() {
    override fun createIntent(context: Context, input: String): Intent =
        Intent(Intent.ACTION_CREATE_DOCUMENT).addCategory(Intent.CATEGORY_OPENABLE).setType(mimeOf(input)).putExtra(Intent.EXTRA_TITLE, input)

    override fun parseResult(resultCode: Int, intent: Intent?): Uri? = intent?.data
}

/** Where the cache's copies go; the file provider hands them out from there. */
private fun cache(context: Context) = File(context.cacheDir, "sftp")

/** A path for [name] in a folder of its own in the cache. */
private fun fresh(context: Context, name: String): File {
    val into = File(cache(context), UUID.randomUUID().toString())
    into.mkdirs()
    return File(into, name)
}

/** Copies over a day old go: another app may still be showing a newer one. */
private fun tidy(context: Context) {
    val old = System.currentTimeMillis() - DateUtils.DAY_IN_MILLIS
    cache(context).listFiles()?.filter { it.lastModified() < old }?.forEach { it.deleteRecursively() }
}

/** Hands [file] to another app through the file provider: to show it, or to send it. */
private fun showOrSend(context: Context, file: File, view: Boolean) {
    val uri = FileProvider.getUriForFile(context, "${context.packageName}.files", file)
    val intent = if (view) {
        Intent(Intent.ACTION_VIEW).setDataAndType(uri, mimeOf(file.name))
    } else {
        Intent(Intent.ACTION_SEND).setType(mimeOf(file.name)).putExtra(Intent.EXTRA_STREAM, uri)
    }
    intent.addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION)
    context.startActivity(Intent.createChooser(intent, file.name).addFlags(Intent.FLAG_ACTIVITY_NEW_TASK))
}

/** Copies [from], a file or a folder, into the folder [parent] that a picker gave. */
private fun copyInto(context: Context, from: File, parent: Uri) {
    val r = context.contentResolver
    if (from.isDirectory) {
        val made = DocumentsContract.createDocument(r, parent, DocumentsContract.Document.MIME_TYPE_DIR, from.name)
            ?: error(tr("Not saved: {error}", "error" to from.name))
        from.listFiles()?.forEach { copyInto(context, it, made) }
    } else {
        val made = DocumentsContract.createDocument(r, parent, mimeOf(from.name), from.name)
            ?: error(tr("Not saved: {error}", "error" to from.name))
        r.openOutputStream(made)?.use { o -> from.inputStream().use { it.copyTo(o) } }
    }
}

/** Whether [name]'s extension says its type: an app can show it. */
private fun known(name: String) = mimeOf(name) != "application/octet-stream"

private fun mimeOf(name: String): String =
    MimeTypeMap.getSingleton().getMimeTypeFromExtension(name.substringAfterLast('.', "").lowercase())
        ?: "application/octet-stream"

/** The name a picker's document goes by. */
private fun displayName(context: Context, uri: Uri): String =
    context.contentResolver.query(uri, arrayOf(OpenableColumns.DISPLAY_NAME), null, null, null)?.use { c ->
        if (c.moveToFirst()) c.getString(0) else null
    }?.takeIf { it.isNotBlank() && !it.contains('/') } ?: (uri.lastPathSegment?.substringAfterLast('/') ?: "file")

/** [bytes] as text when they are UTF-8 and hold no NUL. */
private fun textOf(bytes: ByteArray): String? {
    if (bytes.contains(0)) return null
    return try {
        Charsets.UTF_8.newDecoder()
            .onMalformedInput(CodingErrorAction.REPORT)
            .onUnmappableCharacter(CodingErrorAction.REPORT)
            .decode(ByteBuffer.wrap(bytes)).toString()
    } catch (_: java.nio.charset.CharacterCodingException) {
        null
    }
}

private fun detail(context: Context, e: FileEntry): String {
    val parts = buildList {
        if (!e.dir) add(Formatter.formatShortFileSize(context, e.size.toLong()))
        e.modified?.let {
            add(
                DateUtils.formatDateTime(
                    context, it.toLong() * 1000,
                    DateUtils.FORMAT_SHOW_DATE or DateUtils.FORMAT_SHOW_TIME or DateUtils.FORMAT_ABBREV_MONTH,
                ),
            )
        }
    }
    return if (parts.isEmpty()) (if (e.dir) tr("Folder") else "") else parts.joinToString(" · ")
}

/** [name] in the folder [dir]. */
private fun join(dir: String, name: String) = if (dir.endsWith("/")) dir + name else "$dir/$name"

/** A path's last part; `/` for the root. */
private fun name(path: String): String =
    if (path.isEmpty()) tr("Files") else path.split('/').lastOrNull { it.isNotEmpty() } ?: "/"

/** The folders [path] is in, nearest first, up to `/`. */
private fun parents(path: String): List<String> {
    val parts = path.split('/').filter { it.isNotEmpty() }.toMutableList()
    val out = mutableListOf<String>()
    while (parts.isNotEmpty()) {
        parts.removeAt(parts.lastIndex)
        out.add("/" + parts.joinToString("/"))
    }
    return out
}
