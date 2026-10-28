#!/usr/bin/env python3
"""Check the local GTK's codec linkage and texture round trips without a window."""
import json
import hashlib
import os
from pathlib import Path
import shlex
import subprocess
import sys
import tempfile

prefix, projects = map(lambda path: Path(path).resolve(), sys.argv[1:])
library = prefix / 'lib/libgtk-4.so.1'
libraries = [library, *[prefix / f'lib/lib{name}-2.0.so.0' for name in ['glib', 'gobject', 'gio', 'gmodule', 'gthread']]]
libraries += [prefix / f'lib/lib{name}-1.0.so.0' for name in ['pango', 'pangocairo', 'pangoft2']]
for path in libraries:
    dynamic = subprocess.check_output(['readelf', '-d', str(path)], text=True)
    assert '(RUNPATH)' in dynamic and 'Library runpath: [$ORIGIN]' in dynamic, dynamic
assert str(projects.parent.parent).encode() not in library.read_bytes(), 'GTK library embeds its build directory'
needed = subprocess.check_output(['readelf', '-d', str(library)], text=True)
assert 'Shared library: [libtiff' not in needed, needed
assert 'Shared library: [libjpeg' not in needed, needed
manifest = json.loads((prefix / 'manifest.json').read_text())
assert 'share/doc/capycanvas-gtk/vulkan-memory-pool.patch' in manifest['files']
for filename, digest in manifest['files'].items():
    assert hashlib.sha256((prefix / filename).read_bytes()).hexdigest() == digest, filename
for name in ['libtiff', 'libjpeg-turbo']:
    assert name in manifest['dependencies'], name
    assert f'share/doc/capycanvas-gtk/{name}/LICENSE.md' in manifest['files'], name
assert manifest['dependencies']['pango']['version'] == '1.58.2'
assert 'share/doc/capycanvas-gtk/sources/pango-1.58.2.tar.xz' in manifest['files']
assert 'share/doc/capycanvas-gtk/pango/COPYING' in manifest['files']

source = r'''
#define _GNU_SOURCE
#include <gtk/gtk.h>
#include <stdio.h>
#include <jpeglib.h>
#include <dlfcn.h>

static void check_texture (GBytes *bytes)
{
  GError *error = NULL;
  GdkTexture *texture = gdk_texture_new_from_bytes (bytes, &error);
  g_assert_no_error (error);
  g_assert_nonnull (texture);
  g_assert_cmpint (gdk_texture_get_width (texture), ==, 2);
  g_assert_cmpint (gdk_texture_get_height (texture), ==, 2);
  GdkTextureDownloader *downloader = gdk_texture_downloader_new (texture);
  gdk_texture_downloader_set_format (downloader, GDK_MEMORY_R8G8B8A8);
  guint8 pixels[16];
  gdk_texture_downloader_download_into (downloader, pixels, 8);
  for (int i = 0; i < 16; i += 4)
    {
      g_assert_cmpint (pixels[i], >=, 250);
      g_assert_cmpint (pixels[i + 1], <=, 5);
      g_assert_cmpint (pixels[i + 2], <=, 5);
      g_assert_cmpint (pixels[i + 3], ==, 255);
    }
  gdk_texture_downloader_free (downloader);
  g_object_unref (texture);
  g_bytes_unref (bytes);
}

int main (int argc, char **argv)
{
  Dl_info info;
  g_assert_true (dladdr (dlsym (RTLD_DEFAULT, "gdk_texture_new_from_bytes"), &info));
  g_assert_cmpstr (realpath (info.dli_fname, NULL), ==, argv[1]);
  g_assert_true (dladdr (dlsym (RTLD_DEFAULT, "pango_version"), &info));
  g_assert_cmpstr (realpath (info.dli_fname, NULL), ==, argv[2]);
  g_assert_cmpint (pango_version (), >=, 15802);
  PangoContext *context = pango_font_map_create_context (pango_cairo_font_map_get_default ());
  PangoLayout *layout = pango_layout_new (context);
  pango_layout_set_text (layout, "Paint 日本語 العربية ภาษาไทย", -1);
  int width, height;
  pango_layout_get_pixel_size (layout, &width, &height);
  g_assert_cmpint (width, >, 0);
  g_assert_cmpint (height, >, 0);
  g_object_unref (layout);
  g_object_unref (context);
  const guint8 rgba[] = {255, 0, 0, 255, 255, 0, 0, 255,
                        255, 0, 0, 255, 255, 0, 0, 255};
  GBytes *pixels = g_bytes_new_static (rgba, sizeof rgba);
  GdkTexture *texture = gdk_memory_texture_new (2, 2, GDK_MEMORY_R8G8B8A8, pixels, 8);
  g_bytes_unref (pixels);
  check_texture (gdk_texture_save_to_png_bytes (texture));
  check_texture (gdk_texture_save_to_tiff_bytes (texture));
  g_object_unref (texture);

  struct jpeg_compress_struct encoder;
  struct jpeg_error_mgr errors;
  encoder.err = jpeg_std_error (&errors);
  jpeg_create_compress (&encoder);
  unsigned char *jpeg = NULL;
  unsigned long size = 0;
  jpeg_mem_dest (&encoder, &jpeg, &size);
  encoder.image_width = encoder.image_height = 2;
  encoder.input_components = 3;
  encoder.in_color_space = JCS_RGB;
  jpeg_set_defaults (&encoder);
  jpeg_set_quality (&encoder, 100, TRUE);
  jpeg_start_compress (&encoder, TRUE);
  guint8 rgb[] = {255, 0, 0, 255, 0, 0};
  JSAMPROW row = rgb;
  while (encoder.next_scanline < encoder.image_height)
    jpeg_write_scanlines (&encoder, &row, 1);
  jpeg_finish_compress (&encoder);
  jpeg_destroy_compress (&encoder);
  check_texture (g_bytes_new_take (jpeg, size));
  puts ("PASS: local GTK, static TIFF/JPEG, source checksums, PNG/TIFF/JPEG pixels");
}
'''
flags = shlex.split(subprocess.check_output(['pkg-config', '--cflags', '--libs', 'gtk4'], text=True))
flags = [flag for flag in flags if flag not in ['-lgtk-4', '-lglib-2.0', '-lgobject-2.0', '-lgio-2.0', '-lgmodule-2.0', '-lgthread-2.0',
                                               '-lpango-1.0', '-lpangocairo-1.0', '-lpangoft2-1.0']]
with tempfile.TemporaryDirectory(prefix='capy-gtk-codecs-') as directory:
    test = Path(directory) / 'test.c'
    test.write_text(source)
    executable = Path(directory) / 'test'
    subprocess.run(['cc', str(test), '-o', str(executable),
                    '-I' + str(projects / 'libjpeg-turbo-3.1.1/src'),
                    '-I' + str(projects.parent / '../build/subprojects/libjpeg-turbo-3.1.1/src'),
                    *map(str, libraries), *flags, '-ldl', '-Wl,-rpath,' + str(prefix / 'lib')], check=True)
    env = dict(os.environ)
    env.pop('LD_LIBRARY_PATH', None)
    subprocess.run([str(executable), str(library), str(prefix / 'lib/libpango-1.0.so.0')], env=env, check=True)
    subprocess.run([sys.executable, '-c',
                    """import ctypes, os, pathlib, sys
root = pathlib.Path(sys.argv[1])
names = ['libgtk-4.so.1', *['lib' + name + '-2.0.so.0' for name in ['glib', 'gobject', 'gio', 'gmodule', 'gthread']]]
names += ['lib' + name + '-1.0.so.0' for name in ['pango', 'pangocairo', 'pangoft2']]
for name in names:
    ctypes.CDLL(str(root / name), mode=os.RTLD_NOW | os.RTLD_GLOBAL)
ctypes.CDLL('libadwaita-1.so.0', mode=os.RTLD_NOW)
maps = pathlib.Path('/proc/self/maps').read_text().splitlines()
for name in names:
    loaded = {line.split()[-1] for line in maps if name in line}
    assert loaded == {str(root / name)}, (name, loaded)
""", str(prefix / 'lib')],
                   env=env, check=True)
    print('PASS: private GTK/GLib/Pango, multilingual layout and system libadwaita resolve without LD_LIBRARY_PATH')
