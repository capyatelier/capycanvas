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
needed = subprocess.check_output(['readelf', '-d', str(library)], text=True)
assert 'Shared library: [libtiff' not in needed, needed
assert 'Shared library: [libjpeg' not in needed, needed
manifest = json.loads((prefix / 'manifest.json').read_text())
for filename, digest in manifest['files'].items():
    assert hashlib.sha256((prefix / filename).read_bytes()).hexdigest() == digest, filename
for name in ['libtiff', 'libjpeg-turbo']:
    assert name in manifest['dependencies'], name
    assert f'share/doc/capycanvas-gtk/{name}/LICENSE.md' in manifest['files'], name

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
with tempfile.TemporaryDirectory(prefix='capy-gtk-codecs-') as directory:
    test = Path(directory) / 'test.c'
    test.write_text(source)
    executable = Path(directory) / 'test'
    subprocess.run(['cc', str(test), '-o', str(executable),
                    '-I' + str(projects / 'libjpeg-turbo-3.1.1/src'),
                    '-I' + str(projects.parent / '../build/subprojects/libjpeg-turbo-3.1.1/src'),
                    str(library), *flags, '-ldl'], check=True)
    env = dict(os.environ, LD_LIBRARY_PATH=str(prefix / 'lib'))
    subprocess.run([str(executable), str(library)], env=env, check=True)
