package com.mayorana.scan2fil.platform

import android.app.Activity
import android.content.Intent
import android.os.Bundle
import com.google.mlkit.vision.documentscanner.GmsDocumentScannerOptions
import com.google.mlkit.vision.documentscanner.GmsDocumentScanning
import com.google.mlkit.vision.documentscanner.GmsDocumentScanningResult
import java.io.File

/**
 * Google's document scanner, run by [ScanActivity]: it finds the page's edges,
 * straightens and crops it, and lets the person retake it or import a photo
 * from the gallery. Rust asks for the outcome with [take]: "" while the scanner
 * is open, "cancelled", "file:<path>" of the page as a JPEG, or what went wrong.
 */
internal object Scan {
    @Volatile private var result: String = ""

    fun start(activity: Activity) {
        result = ""
        activity.runOnUiThread {
            activity.startActivity(Intent(activity, ScanActivity::class.java))
        }
    }

    fun finish(answer: String) {
        result = answer
    }

    fun take(): String {
        val r = result
        if (r.isNotEmpty()) result = ""
        return r
    }
}

/**
 * Opens the scanner and hears back from it: the app's own activity belongs to
 * Dioxus, so the scanner's answer comes to this one, which then closes.
 */
class ScanActivity : Activity() {
    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        // Brought back after the system closed it: the scanner is open already.
        if (savedInstanceState != null) return
        val options = GmsDocumentScannerOptions.Builder()
            .setGalleryImportAllowed(true)
            .setPageLimit(1)
            .setResultFormats(GmsDocumentScannerOptions.RESULT_FORMAT_JPEG)
            .setScannerMode(GmsDocumentScannerOptions.SCANNER_MODE_FULL)
            .build()
        GmsDocumentScanning.getClient(options).getStartScanIntent(this)
            .addOnSuccessListener { sender ->
                try {
                    startIntentSenderForResult(sender, REQUEST, null, 0, 0, 0)
                } catch (e: Exception) {
                    done("The scanner could not open: ${e.message}")
                }
            }
            .addOnFailureListener { e ->
                done("This phone's scanner is not available (${e.message}). Choose a photo instead.")
            }
    }

    @Deprecated("Activity results without the AndroidX result API")
    override fun onActivityResult(requestCode: Int, resultCode: Int, data: Intent?) {
        super.onActivityResult(requestCode, resultCode, data)
        if (requestCode != REQUEST) return
        if (resultCode != RESULT_OK) return done("cancelled")
        val uri = GmsDocumentScanningResult.fromActivityResultIntent(data)
            ?.pages?.firstOrNull()?.imageUri
            ?: return done("cancelled")
        val file = File(cacheDir, "scan2fil-scan.jpg")
        try {
            contentResolver.openInputStream(uri).use { input ->
                if (input == null) return done("The scanned page could not be opened.")
                file.outputStream().use { input.copyTo(it) }
            }
        } catch (e: Exception) {
            return done("The scanned page could not be kept: ${e.message}")
        }
        done("file:${file.absolutePath}")
    }

    private fun done(answer: String) {
        Scan.finish(answer)
        finish()
    }

    private companion object {
        const val REQUEST = 4021
    }
}
