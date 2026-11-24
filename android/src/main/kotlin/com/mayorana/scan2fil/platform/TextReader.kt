package com.mayorana.scan2fil.platform

import android.graphics.BitmapFactory
import android.util.Log
import com.google.mlkit.vision.common.InputImage
import com.google.mlkit.vision.text.TextRecognition
import com.google.mlkit.vision.text.latin.TextRecognizerOptions
import org.json.JSONArray
import org.json.JSONObject

/**
 * Google's text reader, on the phone: every word printed on the page with its
 * box, so each answer can be put on the line next to its label. Rust asks for
 * the outcome with [take]: "" while reading, "lines:<json>" (lines of words,
 * each {t, x, y, w, h} in fractions of the page from its top-left), or what
 * went wrong.
 */
internal object TextReader {
    @Volatile private var result: String = ""

    fun start(path: String): String {
        result = ""
        val bitmap = BitmapFactory.decodeFile(path) ?: return "The page to read could not be opened."
        val (w, h) = bitmap.width.toFloat() to bitmap.height.toFloat()
        TextRecognition.getClient(TextRecognizerOptions.DEFAULT_OPTIONS)
            .process(InputImage.fromBitmap(bitmap, 0))
            .addOnSuccessListener { text ->
                val lines = JSONArray()
                for (block in text.textBlocks) {
                    for (line in block.lines) {
                        val words = JSONArray()
                        for (element in line.elements) {
                            val box = element.boundingBox ?: continue
                            words.put(
                                JSONObject()
                                    .put("t", element.text)
                                    .put("x", box.left / w)
                                    .put("y", box.top / h)
                                    .put("w", box.width() / w)
                                    .put("h", box.height() / h)
                            )
                        }
                        lines.put(words)
                    }
                }
                Log.i("scan2fil", "Text reader: ${lines.length()} lines")
                result = "lines:$lines"
            }
            .addOnFailureListener { e ->
                Log.w("scan2fil", "Text reader failed", e)
                // Never empty: that means "still reading" to Rust.
                result = e.message?.takeIf { it.isNotBlank() } ?: e.javaClass.simpleName
            }
        return ""
    }

    fun take(): String {
        val r = result
        if (r.isNotEmpty()) result = ""
        return r
    }
}
