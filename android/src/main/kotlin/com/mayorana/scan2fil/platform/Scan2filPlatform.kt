package com.mayorana.scan2fil.platform

import android.app.Activity
import android.content.Intent
import android.net.Uri
import androidx.browser.customtabs.CustomTabsIntent
import androidx.core.content.FileProvider
import java.io.File

/**
 * scan2fil's Android side, called from Rust (`sheet.rs`). Each call returns
 * an empty string when it worked, or what went wrong in words.
 */
class Scan2filPlatform(private val activity: Activity) {
    /** Opens Android's share sheet for a PDF in the app's cache folder. */
    fun sharePdf(path: String): String {
        val file = File(path)
        if (!file.exists()) {
            return "The PDF to share is missing."
        }
        val uri = try {
            FileProvider.getUriForFile(activity, activity.packageName + ".fileprovider", file)
        } catch (e: IllegalArgumentException) {
            return "The PDF cannot be shared from where it was saved."
        }
        val send = Intent(Intent.ACTION_SEND).apply {
            type = "application/pdf"
            putExtra(Intent.EXTRA_STREAM, uri)
            addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION)
        }
        val chooser = Intent.createChooser(send, file.name).apply {
            addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION)
        }
        activity.runOnUiThread { activity.startActivity(chooser) }
        return ""
    }

    /** Opens the provider's sign-in page in a Custom Tab (the browser if none). */
    fun startSignIn(url: String): String {
        SignIn.start(activity)
        activity.runOnUiThread {
            try {
                CustomTabsIntent.Builder().build().launchUrl(activity, Uri.parse(url))
            } catch (e: Exception) {
                SignIn.finish("No browser could open the sign-in page.")
            }
        }
        return ""
    }

    /** Opens a web page, such as the privacy policy, in a Custom Tab (the browser if none). */
    fun viewUrl(url: String): String {
        activity.runOnUiThread {
            try {
                CustomTabsIntent.Builder().build().launchUrl(activity, Uri.parse(url))
            } catch (e: Exception) {
                // No browser at all: nothing to open the page with.
            }
        }
        return ""
    }

    /** Opens Google's document scanner; see [Scan]. */
    fun startScan(): String {
        Scan.start(activity)
        return ""
    }

    /** "" while the scanner is open; see [Scan.take]. */
    fun scanResult(): String = Scan.take()

    /** Starts reading the text printed on the JPEG at [path]; see [TextReader]. */
    fun startReadText(path: String): String = TextReader.start(path)

    /** "" while the text is still being read; see [TextReader.take]. */
    fun readTextResult(): String = TextReader.take()

    /** "" while the person is still signing in; see [SignIn.result]. */
    fun signInResult(): String = SignIn.take(activity.application)
}
