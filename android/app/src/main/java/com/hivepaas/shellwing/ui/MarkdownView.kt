package com.hivepaas.shellwing.ui

import android.content.ClipData
import android.content.ClipboardManager
import android.graphics.BitmapFactory
import androidx.compose.foundation.Image
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.horizontalScroll
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.IntrinsicSize
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxHeight
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.layout.widthIn
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.text.selection.SelectionContainer
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.CheckBox
import androidx.compose.material.icons.filled.CheckBoxOutlineBlank
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.asImageBitmap
import androidx.compose.ui.layout.ContentScale
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.text.AnnotatedString
import androidx.compose.ui.text.LinkAnnotation
import androidx.compose.ui.text.SpanStyle
import androidx.compose.ui.text.TextLinkStyles
import androidx.compose.ui.text.buildAnnotatedString
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.font.FontStyle
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.text.style.TextDecoration
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import com.hivepaas.shellwing.AppModel
import com.hivepaas.shellwing.core.MdAlert
import com.hivepaas.shellwing.core.MdAlign
import com.hivepaas.shellwing.core.MdBlock
import com.hivepaas.shellwing.core.MdCell
import com.hivepaas.shellwing.core.MdKind
import com.hivepaas.shellwing.core.MdRow
import com.hivepaas.shellwing.core.MdSpan
import com.hivepaas.shellwing.core.markdownBlocks
import com.hivepaas.shellwing.tr

/**
 * A note's Markdown shown, as the desktop shows it: headings, paragraphs,
 * lists and tasks, quotes and alerts, code, tables, images (`attachment:<id>`
 * from the vault), rules. The core reads the Markdown (markdownBlocks); this
 * draws its blocks. A task's box ticks when [toggle] is given (with the box's
 * place in the text); in a session, [insert] puts a code block's commands
 * into its shell. As ios/Shellwing/MarkdownView.swift.
 */
@Composable
fun MarkdownView(
    model: AppModel,
    text: String,
    insert: ((String) -> Unit)? = null,
    toggle: ((ULong) -> Unit)? = null,
) {
    val blocks = remember(text) { markdownBlocks(text) }
    Column(Modifier.fillMaxWidth()) {
        blocks.forEachIndexed { n, block ->
            if (n > 0) Spacer(Modifier.height(gap(blocks[n - 1], block)))
            QuoteBars(block.quote.toInt(), block.alert) {
                Box(Modifier.padding(start = (block.indent.toInt() * 18).dp)) {
                    Block(model, block, insert, toggle)
                }
            }
        }
    }
}

private const val BASE = 15f

/** None inside a quote, whose bar and tint run on; little between a list's items. */
private fun gap(a: MdBlock, b: MdBlock) = when {
    a.group != 0u && a.group == b.group -> 0.dp
    a.kind is MdKind.Item && b.kind is MdKind.Item -> 6.dp
    else -> 10.dp
}

@Composable
private fun Block(model: AppModel, block: MdBlock, insert: ((String) -> Unit)?, toggle: ((ULong) -> Unit)?) {
    when (val k = block.kind) {
        is MdKind.Heading -> SelectionContainer {
            Text(
                styled(k.spans), fontSize = (BASE * scale(k.level.toInt())).sp, fontWeight = FontWeight.SemiBold,
                modifier = Modifier.padding(top = if (k.level <= 2u) 6.dp else 2.dp),
            )
        }
        is MdKind.Paragraph -> SelectionContainer { Text(styled(k.spans), fontSize = BASE.sp) }
        is MdKind.Item -> Row(verticalAlignment = Alignment.Top) {
            val done = k.task
            if (done != null) {
                val at = k.taskAt
                IconButton(
                    onClick = { if (at != null) toggle?.invoke(at) },
                    enabled = toggle != null && at != null,
                    modifier = Modifier.width(28.dp).height(22.dp),
                ) {
                    Icon(
                        if (done) Icons.Filled.CheckBox else Icons.Filled.CheckBoxOutlineBlank,
                        if (done) tr("Done") else tr("Not done"),
                        tint = if (done) Dusk.accent else Dusk.muted,
                    )
                }
            } else {
                Text(k.marker, color = Dusk.muted, fontSize = BASE.sp)
            }
            Spacer(Modifier.width(8.dp))
            SelectionContainer {
                Text(
                    styled(k.spans), fontSize = BASE.sp,
                    color = if (done == true) Dusk.muted else Dusk.text,
                    textDecoration = if (done == true) TextDecoration.LineThrough else null,
                )
            }
        }
        is MdKind.Code -> CodeBlock(k.lang, k.text, insert)
        is MdKind.Table -> Table(k.aligns, k.head, k.rows)
        is MdKind.Image -> NoteImage(model, k.source, k.alt)
        is MdKind.Rule -> HorizontalDivider(Modifier.padding(vertical = 4.dp), color = Dusk.line)
        is MdKind.AlertTitle -> Text(
            alertTitle(k.kind), color = alertColor(k.kind), fontSize = BASE.sp, fontWeight = FontWeight.SemiBold,
        )
    }
}

private fun scale(level: Int) = when (level) {
    1 -> 1.8f
    2 -> 1.5f
    3 -> 1.3f
    4 -> 1.15f
    5 -> 1.05f
    else -> 1f
}

/** Spans as one text: bold, italic, struck, code, links that open. */
private fun styled(spans: List<MdSpan>): AnnotatedString = buildAnnotatedString {
    for (s in spans) {
        val style = SpanStyle(
            fontWeight = if (s.bold) FontWeight.Bold else null,
            fontStyle = if (s.italic) FontStyle.Italic else null,
            textDecoration = if (s.strike) TextDecoration.LineThrough else null,
            fontFamily = if (s.code) FontFamily.Monospace else null,
            background = if (s.code) Dusk.field else Color.Unspecified,
        )
        val link = s.link
        if (link != null) {
            pushLink(
                LinkAnnotation.Url(
                    link,
                    TextLinkStyles(SpanStyle(color = Dusk.accent, textDecoration = TextDecoration.Underline)),
                ),
            )
        }
        pushStyle(style)
        append(s.text)
        pop()
        if (link != null) pop()
    }
}

@Composable
private fun Table(aligns: List<MdAlign>, head: List<MdCell>, rows: List<MdRow>) {
    val shape = RoundedCornerShape(6.dp)
    Box(Modifier.horizontalScroll(rememberScrollState())) {
        Column(Modifier.clip(shape).border(1.dp, Dusk.line, shape)) {
            Row(Modifier.background(Dusk.raised).height(IntrinsicSize.Min)) {
                head.forEachIndexed { n, cell -> Cell(cell, aligns.getOrNull(n), bold = true) }
            }
            for (row in rows) {
                HorizontalDivider(color = Dusk.line)
                Row(Modifier.height(IntrinsicSize.Min)) {
                    row.cells.forEachIndexed { n, cell -> Cell(cell, aligns.getOrNull(n), bold = false) }
                }
            }
        }
    }
}

@Composable
private fun Cell(cell: MdCell, align: MdAlign?, bold: Boolean) {
    Row(Modifier.fillMaxHeight()) {
        Text(
            styled(cell.spans), fontSize = (BASE - 1).sp,
            fontWeight = if (bold) FontWeight.SemiBold else null,
            textAlign = when (align) {
                MdAlign.RIGHT -> TextAlign.End
                MdAlign.CENTER -> TextAlign.Center
                else -> TextAlign.Start
            },
            modifier = Modifier.widthIn(min = 60.dp, max = 280.dp).padding(horizontal = 10.dp, vertical = 7.dp),
        )
        Box(Modifier.width(1.dp).fillMaxHeight().background(Dusk.line))
    }
}

@Composable
private fun NoteImage(model: AppModel, source: String, alt: String) {
    val id = source.removePrefix("attachment:").takeIf { source.startsWith("attachment:") }
    val bitmap = remember(id) {
        id?.let { model.core?.noteImage(it) }?.let { BitmapFactory.decodeByteArray(it, 0, it.size) }
    }
    if (bitmap != null) {
        Image(
            bitmap.asImageBitmap(), alt.ifEmpty { tr("Image") },
            contentScale = ContentScale.Fit,
            modifier = Modifier.fillMaxWidth().heightIn(max = 420.dp).clip(RoundedCornerShape(6.dp)),
        )
    } else {
        Text(alt.ifEmpty { tr("The image cannot be shown") }, fontSize = 13.sp, color = Dusk.muted)
    }
}

/** A code block: its language, Copy, and Insert in a session. */
@Composable
private fun CodeBlock(lang: String, code: String, insert: ((String) -> Unit)?) {
    val context = LocalContext.current
    var copied by remember { mutableStateOf(false) }
    val shape = RoundedCornerShape(8.dp)
    Column(Modifier.fillMaxWidth().clip(shape).background(Dusk.field).border(1.dp, Dusk.line, shape)) {
        Row(Modifier.fillMaxWidth().padding(start = 10.dp), verticalAlignment = Alignment.CenterVertically) {
            Text(lang, fontSize = 11.sp, color = Dusk.muted, modifier = Modifier.weight(1f))
            TextButton(onClick = {
                context.getSystemService(ClipboardManager::class.java)
                    ?.setPrimaryClip(ClipData.newPlainText(lang, code))
                copied = true
            }) { Text(if (copied) tr("Copied") else tr("Copy")) }
            if (insert != null) TextButton(onClick = { insert(code) }) { Text(tr("Insert")) }
        }
        SelectionContainer(Modifier.horizontalScroll(rememberScrollState())) {
            Text(
                code, fontFamily = FontFamily.Monospace, fontSize = 13.sp, color = Dusk.text,
                softWrap = false, modifier = Modifier.padding(10.dp),
            )
        }
    }
}

/** The bars of the quotes a block is in; an alert's in its color, its background tinted. */
@Composable
private fun QuoteBars(depth: Int, alert: MdAlert?, content: @Composable () -> Unit) {
    if (depth == 0) {
        content()
        return
    }
    Row(
        Modifier.fillMaxWidth().height(IntrinsicSize.Min)
            .background(alert?.let { alertColor(it).copy(alpha = 0.08f) } ?: Color.Transparent),
        horizontalArrangement = Arrangement.spacedBy(10.dp),
    ) {
        repeat(depth) { n ->
            val color = if (n == depth - 1) alert?.let { alertColor(it) } ?: Dusk.edge else Dusk.edge
            Box(Modifier.width(3.dp).fillMaxHeight().background(color))
        }
        Box(Modifier.padding(vertical = 5.dp).weight(1f)) { content() }
    }
}

private fun alertTitle(a: MdAlert) = when (a) {
    MdAlert.NOTE -> tr("Note")
    MdAlert.TIP -> tr("Tip")
    MdAlert.IMPORTANT -> tr("Important")
    MdAlert.WARNING -> tr("Warning")
    MdAlert.CAUTION -> tr("Caution")
}

private fun alertColor(a: MdAlert) = when (a) {
    MdAlert.NOTE -> Dusk.accent
    MdAlert.TIP -> Dusk.ok
    MdAlert.IMPORTANT -> rgb(0xa78bfa)
    MdAlert.WARNING -> Dusk.amber
    MdAlert.CAUTION -> Dusk.bad
}
