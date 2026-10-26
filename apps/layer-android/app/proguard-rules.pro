# Rust exports Java_art_capycanvas_Native_* entry points by their JNI names.
-keep class art.capycanvas.Native { *; }
-keep class art.capycanvas.PhotoReader { public static boolean decode(int, int, int, long, long, int); }
