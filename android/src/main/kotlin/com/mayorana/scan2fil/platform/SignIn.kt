package com.mayorana.scan2fil.platform

import android.app.Activity
import android.app.Application
import android.content.Intent
import android.os.Bundle

/**
 * One sign-in at a time: the browser opens the provider's page, the provider
 * redirects to scan2fil's scheme, [SignInRedirectActivity] catches that and
 * the answer waits here until Rust asks for it.
 */
internal object SignIn {
    /** "" while waiting; then "url:<the redirect>", "cancelled" or a problem. */
    @Volatile var result: String = ""
    @Volatile var waiting: Boolean = false
    private var watcher: Application.ActivityLifecycleCallbacks? = null

    fun start(activity: Activity) {
        stopWatching(activity.application)
        result = ""
        waiting = true
        // Back on the app's screen with no redirect: the person closed the browser.
        var left = false
        val callbacks = object : Application.ActivityLifecycleCallbacks {
            override fun onActivityPaused(a: Activity) {
                if (a === activity) left = true
            }
            override fun onActivityResumed(a: Activity) {
                if (a === activity && left && waiting) finish("cancelled")
            }
            override fun onActivityCreated(a: Activity, b: Bundle?) {}
            override fun onActivityStarted(a: Activity) {}
            override fun onActivityStopped(a: Activity) {}
            override fun onActivitySaveInstanceState(a: Activity, b: Bundle) {}
            override fun onActivityDestroyed(a: Activity) {}
        }
        watcher = callbacks
        activity.application.registerActivityLifecycleCallbacks(callbacks)
    }

    fun finish(answer: String) {
        if (!waiting) return
        result = answer
        waiting = false
    }

    /** Hands the answer over once, then forgets it. */
    fun take(app: Application): String {
        val answer = result
        if (answer.isNotEmpty()) {
            result = ""
            stopWatching(app)
        }
        return answer
    }

    private fun stopWatching(app: Application) {
        watcher?.let { app.unregisterActivityLifecycleCallbacks(it) }
        watcher = null
    }
}

/** Receives the provider's redirect, then brings the app's screen back. */
class SignInRedirectActivity : Activity() {
    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        intent?.data?.let { SignIn.finish("url:$it") }
        packageManager.getLaunchIntentForPackage(packageName)?.let {
            it.addFlags(Intent.FLAG_ACTIVITY_CLEAR_TOP or Intent.FLAG_ACTIVITY_SINGLE_TOP)
            startActivity(it)
        }
        finish()
    }
}
