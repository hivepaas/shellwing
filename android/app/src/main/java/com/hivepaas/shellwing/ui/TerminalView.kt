package com.hivepaas.shellwing.ui

import android.annotation.SuppressLint
import android.content.Context
import android.graphics.Canvas
import android.graphics.Paint
import android.graphics.Typeface
import android.text.InputType
import android.util.TypedValue
import android.view.GestureDetector
import android.view.KeyEvent
import android.view.MotionEvent
import android.view.ScaleGestureDetector
import android.view.View
import android.view.inputmethod.BaseInputConnection
import android.view.inputmethod.EditorInfo
import android.view.inputmethod.InputConnection
import android.view.inputmethod.InputMethodManager
import com.hivepaas.shellwing.SessionModel
import com.hivepaas.shellwing.core.FrameData
import com.hivepaas.shellwing.core.Key
import com.hivepaas.shellwing.core.RunData
import com.hivepaas.shellwing.tr
import java.text.BreakIterator
import kotlin.math.ceil
import kotlin.math.max

/**
 * The terminal: draws a frame cell by cell (runs of styled text, their
 * backgrounds, the cursor) and passes what is typed to the session, from
 * the on-screen keyboard or a physical one. As the iOS app's TerminalCanvas.
 */
@SuppressLint("ViewConstructor")
class TerminalView(context: Context, private val onFocus: (Boolean) -> Unit) : View(context) {
    var model: SessionModel? = null
    var frame: FrameData? = null
        set(value) {
            if (value?.seq != field?.seq) {
                field = value
                invalidate()
            }
        }

    /** Text size in sp; a pinch changes it, kept for next time. */
    var fontSize: Float = DEFAULT_SIZE
        set(value) {
            if (value == field) return
            field = value
            measureCells()
            size = 0 to 0
            requestLayout()
            sendSize()
            invalidate()
        }
    var onFontSize: (Float) -> Unit = {}

    private val regular = Paint(Paint.ANTI_ALIAS_FLAG).apply { typeface = Typeface.MONOSPACE }
    private val bold = Paint(Paint.ANTI_ALIAS_FLAG).apply {
        typeface = Typeface.create(Typeface.MONOSPACE, Typeface.BOLD)
    }
    private val fill = Paint()
    private var cellW = 8f
    private var cellH = 16f
    private var baseline = 12f
    private val inset = dp(4f)
    private var size = 0 to 0
    /** Rows of the history scrolled but not yet sent, a drag's leftover. */
    private var scrollRest = 0f
    private var pinchedFrom = fontSize
    private val graphemes = BreakIterator.getCharacterInstance()

    init {
        isFocusable = true
        isFocusableInTouchMode = true
        contentDescription = tr("Terminal")
        measureCells()
    }

    private fun dp(v: Float) = TypedValue.applyDimension(TypedValue.COMPLEX_UNIT_DIP, v, resources.displayMetrics)

    private fun measureCells() {
        val px = TypedValue.applyDimension(TypedValue.COMPLEX_UNIT_SP, fontSize, resources.displayMetrics)
        regular.textSize = px
        bold.textSize = px
        cellW = regular.measureText("M")
        val m = regular.fontMetrics
        cellH = ceil(m.descent - m.ascent + dp(2f))
        baseline = dp(1f) - m.ascent
    }

    // Size

    override fun onSizeChanged(w: Int, h: Int, oldw: Int, oldh: Int) {
        super.onSizeChanged(w, h, oldw, oldh)
        sendSize()
    }

    private fun sendSize() {
        if (width == 0 || height == 0) return
        val cols = max(10, ((width - 2 * inset) / cellW).toInt())
        val rows = max(4, ((height - 2 * inset) / cellH).toInt())
        if (cols to rows == size) return
        size = cols to rows
        model?.resize(cols, rows, cellW.toInt(), cellH.toInt())
    }

    // Drawing

    override fun onDraw(canvas: Canvas) {
        val f = frame
        canvas.drawColor(rgbInt(f?.background ?: 0x272e38u))
        if (f == null) return
        for ((y, row) in f.rows.withIndex()) {
            val top = inset + y * cellH
            for (run in row.runs) {
                val x = inset + run.col.toInt() * cellW
                val w = run.cells.toInt() * cellW
                run.bg?.let {
                    fill.color = rgbInt(it)
                    canvas.drawRect(x, top, x + w, top + cellH, fill)
                }
                if (run.selected) {
                    fill.color = 0x594cc4f9
                    canvas.drawRect(x, top, x + w, top + cellH, fill)
                }
                if (run.text.isBlank()) continue
                val paint = if (run.bold) bold else regular
                paint.color = rgbInt(run.fg)
                paint.isUnderlineText = run.underline
                paint.textSkewX = if (run.italic) -0.18f else 0f
                drawCells(canvas, run, paint, x, top + baseline)
            }
        }
        val col = f.cursorCol
        val row = f.cursorRow
        if (col != null && row != null && !f.scrolledBack) {
            val x = inset + col.toInt() * cellW
            val y = inset + row.toInt() * cellH
            fill.color = rgbInt(f.cursorColor)
            if (hasFocus()) {
                fill.alpha = 0xd9
                fill.style = Paint.Style.FILL
            } else {
                fill.style = Paint.Style.STROKE
                fill.strokeWidth = dp(1f)
            }
            canvas.drawRect(x, y, x + cellW, y + cellH, fill)
            fill.style = Paint.Style.FILL
            fill.alpha = 0xff
        }
    }

    /**
     * A run's text, one grapheme per cell, so a font's fallback (CJK,
     * emoji) cannot push the columns after it out of line.
     */
    private fun drawCells(canvas: Canvas, run: RunData, paint: Paint, x: Float, y: Float) {
        val text = run.text
        if (run.wide || text.all { it.code < 128 }) {
            canvas.drawText(text, x, y, paint)
            return
        }
        graphemes.setText(text)
        var start = graphemes.first()
        var at = x
        var end = graphemes.next()
        while (end != BreakIterator.DONE) {
            canvas.drawText(text, start, end, at, y, paint)
            at += cellW
            start = end
            end = graphemes.next()
        }
    }

    // Gestures: a tap raises the keyboard (or lowers it), a drag scrolls the
    // history, a pinch sizes the text.

    private val gestures = GestureDetector(context, object : GestureDetector.SimpleOnGestureListener() {
        override fun onDown(e: MotionEvent) = true

        override fun onSingleTapUp(e: MotionEvent): Boolean {
            if (hasFocus() && keyboardUp) hideKeyboard() else showKeyboard()
            return true
        }

        override fun onScroll(e1: MotionEvent?, e2: MotionEvent, dx: Float, dy: Float): Boolean {
            // A finger moving down shows what came before: up the history.
            scrollRest -= dy / cellH
            val lines = scrollRest.toInt()
            if (lines != 0) {
                scrollRest -= lines
                model?.scroll(lines.toFloat())
            }
            return true
        }
    })

    private val pinch = ScaleGestureDetector(context, object : ScaleGestureDetector.SimpleOnScaleGestureListener() {
        override fun onScaleBegin(detector: ScaleGestureDetector): Boolean {
            pinchedFrom = fontSize
            return true
        }

        override fun onScale(detector: ScaleGestureDetector): Boolean {
            pinchedFrom *= detector.scaleFactor
            val next = (Math.round(pinchedFrom * 2) / 2f).coerceIn(MIN_SIZE, MAX_SIZE)
            if (next != fontSize) {
                fontSize = next
                onFontSize(next)
            }
            return true
        }
    })

    @SuppressLint("ClickableViewAccessibility")
    override fun onTouchEvent(event: MotionEvent): Boolean {
        pinch.onTouchEvent(event)
        if (!pinch.isInProgress) gestures.onTouchEvent(event)
        if (event.actionMasked == MotionEvent.ACTION_UP) scrollRest = 0f
        return true
    }

    // The keyboard

    private var keyboardUp = false

    fun showKeyboard() {
        requestFocus()
        context.getSystemService(InputMethodManager::class.java)?.showSoftInput(this, 0)
        keyboardUp = true
        onFocus(true)
    }

    fun hideKeyboard() {
        context.getSystemService(InputMethodManager::class.java)?.hideSoftInputFromWindow(windowToken, 0)
        keyboardUp = false
        onFocus(false)
    }

    override fun onFocusChanged(gained: Boolean, direction: Int, previous: android.graphics.Rect?) {
        super.onFocusChanged(gained, direction, previous)
        invalidate()
    }

    override fun onCheckIsTextEditor() = true

    override fun onCreateInputConnection(info: EditorInfo): InputConnection {
        // No suggestions, no autocorrect, no capitals: what is typed goes as typed.
        info.inputType = InputType.TYPE_CLASS_TEXT or InputType.TYPE_TEXT_VARIATION_VISIBLE_PASSWORD or
            InputType.TYPE_TEXT_FLAG_NO_SUGGESTIONS
        info.imeOptions = EditorInfo.IME_FLAG_NO_EXTRACT_UI or EditorInfo.IME_FLAG_NO_FULLSCREEN or
            EditorInfo.IME_ACTION_NONE
        return Typing()
    }

    /**
     * What the on-screen keyboard sends: text, and deletes. Text being
     * composed (an input method's) goes when it is committed.
     */
    private inner class Typing : BaseInputConnection(this, false) {
        override fun commitText(text: CharSequence, newCursorPosition: Int): Boolean {
            send(text.toString())
            return true
        }

        override fun finishComposingText(): Boolean {
            val composing = editable?.let { getComposingSpanText(it) }
            if (!composing.isNullOrEmpty()) send(composing)
            editable?.clear()
            return true
        }

        override fun setComposingText(text: CharSequence, newCursorPosition: Int): Boolean {
            editable?.replace(0, editable!!.length, text)
            setComposingRegion(0, text.length)
            return true
        }

        override fun deleteSurroundingText(before: Int, after: Int): Boolean {
            repeat(max(before, 1)) { model?.press(Key.Backspace) }
            return true
        }

        override fun sendKeyEvent(event: KeyEvent): Boolean {
            if (event.action == KeyEvent.ACTION_DOWN) onKeyDown(event.keyCode, event)
            return true
        }

        private fun getComposingSpanText(e: android.text.Editable): String? {
            val start = getComposingSpanStart(e)
            val end = getComposingSpanEnd(e)
            return if (start in 0..<end) e.subSequence(start, end).toString() else null
        }
    }

    private fun send(text: String) {
        if (text.isEmpty()) return
        model?.type(text)
    }

    /** A physical keyboard's keys, and the on-screen one's Enter and delete. */
    override fun onKeyDown(keyCode: Int, event: KeyEvent): Boolean {
        val model = model ?: return super.onKeyDown(keyCode, event)
        val ctrl = event.isCtrlPressed || model.ctrl
        val alt = event.isAltPressed || model.alt
        val shift = event.isShiftPressed
        val key = when (keyCode) {
            KeyEvent.KEYCODE_ENTER, KeyEvent.KEYCODE_NUMPAD_ENTER -> Key.Enter
            KeyEvent.KEYCODE_DEL -> Key.Backspace
            KeyEvent.KEYCODE_FORWARD_DEL -> Key.Delete
            KeyEvent.KEYCODE_TAB -> Key.Tab
            KeyEvent.KEYCODE_ESCAPE -> Key.Escape
            KeyEvent.KEYCODE_DPAD_UP -> Key.Up
            KeyEvent.KEYCODE_DPAD_DOWN -> Key.Down
            KeyEvent.KEYCODE_DPAD_LEFT -> Key.Left
            KeyEvent.KEYCODE_DPAD_RIGHT -> Key.Right
            KeyEvent.KEYCODE_MOVE_HOME -> Key.Home
            KeyEvent.KEYCODE_MOVE_END -> Key.End
            KeyEvent.KEYCODE_PAGE_UP -> Key.PageUp
            KeyEvent.KEYCODE_PAGE_DOWN -> Key.PageDown
            else -> null
        }
        if (key != null) {
            model.press(key, ctrl, alt, shift)
            return true
        }
        // A letter with Ctrl or Alt: Ctrl+C.
        val plain = event.getUnicodeChar(0)
        if ((ctrl || alt) && plain != 0 && Character.isLetter(plain) && plain < 128) {
            model.press(Key.Letter(plain.toChar().toString()), ctrl, alt, shift)
            return true
        }
        val c = event.getUnicodeChar(event.metaState and KeyEvent.META_SHIFT_MASK)
        if (c != 0 && !Character.isISOControl(c)) {
            model.type(String(Character.toChars(c)))
            return true
        }
        return super.onKeyDown(keyCode, event)
    }

    companion object {
        const val DEFAULT_SIZE = 12.5f
        const val MIN_SIZE = 8f
        const val MAX_SIZE = 24f
        const val SIZE_KEY = "terminal.fontSize"
    }
}

/** `0xrrggbb` as an Android color int, opaque. */
fun rgbInt(hex: UInt): Int = (0xff000000u or hex).toInt()
