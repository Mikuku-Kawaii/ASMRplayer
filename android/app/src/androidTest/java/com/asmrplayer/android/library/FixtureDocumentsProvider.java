package com.asmrplayer.android.library;

import android.content.ContentProvider;
import android.content.ContentValues;
import android.content.Intent;
import android.database.Cursor;
import android.database.MatrixCursor;
import android.net.Uri;
import android.os.Bundle;
import android.os.ParcelFileDescriptor;
import android.provider.DocumentsContract;
import java.io.File;
import java.io.FileOutputStream;
import java.io.FileNotFoundException;
import java.io.IOException;
import java.nio.ByteBuffer;
import java.nio.ByteOrder;
import java.nio.charset.StandardCharsets;
import java.util.Arrays;
import java.util.List;

/** Standalone test-APK process: deliberately uses only platform/Java classes, no target-APK runtime. */
public final class FixtureDocumentsProvider extends ContentProvider {
    private volatile String mode = "normal";
    @Override public boolean onCreate() { return true; }
    @Override public Bundle call(String method, String arg, Bundle extras) {
        if (method.equals("mode")) mode = arg == null ? "normal" : arg;
        if (method.equals("grantRoot")) {
            try {
                String authority = getContext().getPackageManager().getProviderInfo(
                    new android.content.ComponentName(getContext(), FixtureDocumentsProvider.class), 0).authority;
                getContext().grantUriPermission(arg, Uri.parse("content://" + authority + "/"),
                    Intent.FLAG_GRANT_READ_URI_PERMISSION | Intent.FLAG_GRANT_PERSISTABLE_URI_PERMISSION | Intent.FLAG_GRANT_PREFIX_URI_PERMISSION);
            } catch (android.content.pm.PackageManager.NameNotFoundException e) { throw new IllegalStateException(e); }
        }
        if (method.equals("grant")) getContext().grantUriPermission(arg, Uri.parse(extras.getString("uri")),
            Intent.FLAG_GRANT_READ_URI_PERMISSION | Intent.FLAG_GRANT_PERSISTABLE_URI_PERMISSION | Intent.FLAG_GRANT_PREFIX_URI_PERMISSION);
        return Bundle.EMPTY;
    }
    @Override public Cursor query(Uri uri, String[] projection, String selection, String[] selectionArgs, String sortOrder) {
        if (mode.equals("denied")) throw new SecurityException("Synthetic revoked grant");
        String[] columns = projection == null ? new String[] { DocumentsContract.Document.COLUMN_DOCUMENT_ID,
            DocumentsContract.Document.COLUMN_DISPLAY_NAME, DocumentsContract.Document.COLUMN_MIME_TYPE } : projection;
        MatrixCursor cursor = new MatrixCursor(columns);
        String id = DocumentsContract.getDocumentId(uri);
        int slash = id.indexOf('/');
        String root = slash < 0 ? id : id.substring(0, slash);
        String path = slash < 0 ? "" : id.substring(slash + 1);
        boolean children = "children".equals(uri.getLastPathSegment());
        List<String> directories = Arrays.asList("", "WAV", "MP3", "字幕");
        List<String> paths = Arrays.asList("", "WAV", "MP3", "字幕", "WAV/01.wav", "WAV/02.wav",
            "MP3/01.mp3", "WAV/01.ja.srt", "字幕/01.zh.srt", "cover.jpg", "unfinished.wav.part");
        if (mode.equals("slow") && children) {
            try { Thread.sleep(600); } catch (InterruptedException e) { Thread.currentThread().interrupt(); }
        }
        if (mode.equals("loading") && children) {
            Bundle extras = new Bundle(); extras.putBoolean(DocumentsContract.EXTRA_LOADING, true); cursor.setExtras(extras);
        }
        for (String p : paths) {
            int lastSlash = p.lastIndexOf('/');
            String parent = lastSlash < 0 ? "" : p.substring(0, lastSlash);
            boolean include = children ? !p.isEmpty() && parent.equals(path) && !(mode.equals("missing") && p.equals("WAV/02.wav")) : p.equals(path);
            if (!include) continue;
            Object[] row = new Object[columns.length];
            for (int i = 0; i < columns.length; i++) {
                switch (columns[i]) {
                    case DocumentsContract.Document.COLUMN_DOCUMENT_ID: row[i] = p.isEmpty() ? root : root + "/" + p; break;
                    case DocumentsContract.Document.COLUMN_DISPLAY_NAME: row[i] = p.isEmpty() ? "RJ123456 Fixture" : p.substring(lastSlash + 1); break;
                    case DocumentsContract.Document.COLUMN_MIME_TYPE: row[i] = directories.contains(p) ? DocumentsContract.Document.MIME_TYPE_DIR : "application/octet-stream"; break;
                    case DocumentsContract.Document.COLUMN_SIZE: row[i] = 1234L; break;
                    case DocumentsContract.Document.COLUMN_LAST_MODIFIED: row[i] = 1000L; break;
                    default: row[i] = null;
                }
            }
            cursor.addRow(row);
        }
        return cursor;
    }
    @Override public ParcelFileDescriptor openFile(Uri uri, String accessMode) throws FileNotFoundException {
        if (mode.equals("denied")) throw new SecurityException("Synthetic revoked grant");
        if (DocumentsContract.getDocumentId(uri).equals("backup.json")) {
            File backup = new File(getContext().getCacheDir(), "backup.json");
            return ParcelFileDescriptor.open(backup, accessMode.contains("w")
                ? ParcelFileDescriptor.MODE_CREATE | ParcelFileDescriptor.MODE_TRUNCATE | ParcelFileDescriptor.MODE_WRITE_ONLY
                : ParcelFileDescriptor.MODE_READ_ONLY);
        }
        boolean subtitle = DocumentsContract.getDocumentId(uri).endsWith(".srt");
        File file = new File(getContext().getCacheDir(), subtitle ? "fixture.srt" : "fixture.wav");
        if (!file.exists()) {
            try (FileOutputStream out = new FileOutputStream(file)) {
                if (subtitle) out.write("1\n00:00:00,000 --> 00:00:02,000\n原文\n译文\n".getBytes(StandardCharsets.UTF_8));
                else {
                    int dataSize = 48000 * 4 * 4;
                    ByteBuffer buffer = ByteBuffer.allocate(44 + dataSize).order(ByteOrder.LITTLE_ENDIAN);
                    buffer.put("RIFF".getBytes(StandardCharsets.US_ASCII)).putInt(dataSize + 36)
                        .put("WAVEfmt ".getBytes(StandardCharsets.US_ASCII)).putInt(16).putShort((short) 1).putShort((short) 2)
                        .putInt(48000).putInt(192000).putShort((short) 4).putShort((short) 16)
                        .put("data".getBytes(StandardCharsets.US_ASCII)).putInt(dataSize);
                    out.write(buffer.array());
                }
            } catch (IOException e) { throw new FileNotFoundException(e.getMessage()); }
        }
        return ParcelFileDescriptor.open(file, ParcelFileDescriptor.MODE_READ_ONLY);
    }
    @Override public String getType(Uri uri) { return "application/octet-stream"; }
    @Override public Uri insert(Uri uri, ContentValues values) { throw new UnsupportedOperationException(); }
    @Override public int delete(Uri uri, String selection, String[] args) { throw new UnsupportedOperationException(); }
    @Override public int update(Uri uri, ContentValues values, String selection, String[] args) { throw new UnsupportedOperationException(); }
}
