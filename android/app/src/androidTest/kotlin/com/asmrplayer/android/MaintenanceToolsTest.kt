package com.asmrplayer.android

import android.content.Intent
import android.net.Uri
import android.os.Bundle
import android.provider.DocumentsContract
import androidx.test.ext.junit.runners.AndroidJUnit4
import androidx.test.platform.app.InstrumentationRegistry
import com.asmrplayer.android.backup.ImportPolicy
import com.asmrplayer.android.data.*
import kotlinx.coroutines.*
import kotlinx.coroutines.flow.first
import org.junit.Assert.*
import org.junit.Test
import org.junit.runner.RunWith

@RunWith(AndroidJUnit4::class)
class MaintenanceToolsTest {
    private val instrumentation = InstrumentationRegistry.getInstrumentation()
    private val context = instrumentation.targetContext
    private val backend = AndroidBackend.get(context)
    private val provider = Uri.parse("content://com.asmrplayer.android.test.fixture")
    private fun main(block: () -> Unit) = instrumentation.runOnMainSync(block)
    private suspend fun done(tools: MaintenanceTools): MaintenanceState = withTimeout(15000) { tools.state.first { !it.busy } }
    private fun mode(value: String) { context.contentResolver.call(provider, "mode", value, null) }

    @Test fun exportPreviewConflictPolicyAndInvalidInputAreReflectedInUiState() = runBlocking {
        val scope = CoroutineScope(SupervisorJob() + Dispatchers.Main)
        var saved = 0
        val tools = MaintenanceTools(context, scope) { saved++ }
        val id = "maintenance-${System.nanoTime()}"
        val file = DocumentsContract.buildDocumentUri(provider.authority!!, "backup.json")
        mode("normal")
        backend.database.library().putWork(LibraryWork(id, null, id, "Maintenance fixture", favorite = true))
        try {
            main { tools.export(file) }
            assertNull(done(tools).error); assertEquals(1, saved)
            backend.library.setFavorite(id, false)
            main { tools.prepare(file) }
            assertEquals(4, done(tools).preview!!.version)
            assertFalse(backend.library.work(id)!!.favorite) // Preview has no side effects.
            main { tools.applyImport(ImportPolicy.KEEP_EXISTING) }
            assertNull(done(tools).error); assertFalse(backend.library.work(id)!!.favorite)
            main { tools.prepare(file) }; done(tools)
            main { tools.applyImport(ImportPolicy.REPLACE_USER_DATA) }
            assertNull(done(tools).error); assertTrue(backend.library.work(id)!!.favorite)
            main { tools.prepare(file) }; done(tools)
            main { tools.cancelImport() }; assertNull(tools.state.value.preview)
            context.contentResolver.openOutputStream(file)!!.use { it.write("invalid JSON".toByteArray()) }
            main { tools.prepare(file) }
            assertNotNull(done(tools).error); assertNull(tools.state.value.preview)
            assertTrue(backend.library.work(id)!!.favorite)
            main { tools.refreshStorage() }
            assertNotNull(done(tools).storage); assertNull(tools.state.value.error)
        } finally { scope.cancel() }
    }

    @Test fun relinkPreviewDoesNotChangeBindingsAndPartialRestoreRequiresConsent() = runBlocking {
        val scope = CoroutineScope(SupervisorJob() + Dispatchers.Main)
        val tools = MaintenanceTools(context, scope) { }
        val id = "maintenance-root-${System.nanoTime()}"
        val old = DocumentsContract.buildTreeDocumentUri(provider.authority!!, "maintenanceOld")
        val replacement = DocumentsContract.buildTreeDocumentUri(provider.authority!!, "maintenanceNew")
        mode("normal")
        backend.database.library().putRoot(LibraryRoot(id, old.toString(), "Maintenance directory", "WORK"))
        backend.library.scan(id)
        context.contentResolver.call(provider, "grant", context.packageName, Bundle().apply { putString("uri", replacement.toString()) })
        try {
            main { tools.refreshRelinks() }
            val title = backend.database.library().root(id)!!.title
            assertTrue(done(tools).relinks.any { it.id == id && it.title == title && it.kind == "directory" })
            mode("missing")
            main { tools.previewRoot(id, replacement) }
            assertEquals(1, done(tools).rootPreview!!.missingPaths.size)
            assertEquals(old.toString(), backend.database.library().root(id)!!.treeUri)
            main { tools.confirmRoot(false) }
            assertNotNull(done(tools).error)
            assertEquals(old.toString(), backend.database.library().root(id)!!.treeUri)
            main { tools.confirmRoot(true) }
            assertNull(done(tools).error); assertNull(tools.state.value.rootPreview)
            assertEquals(replacement.toString(), backend.database.library().root(id)!!.treeUri)
        } finally {
            scope.cancel(); mode("normal")
            try { context.contentResolver.releasePersistableUriPermission(replacement, Intent.FLAG_GRANT_READ_URI_PERMISSION) } catch (_: SecurityException) { }
        }
    }
}
