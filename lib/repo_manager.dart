import 'dart:io';
import 'dart:convert';
import 'package:path/path.dart' as p;
import 'package:http/http.dart' as http;

import 'global_settings.dart';

class RepoManager {
  final Directory rootDir;

  // Reserved directories that should not be treated as Hugging Face users
  static const Set<String> _reservedDirs = {
    'configs',
    'cache',
    'temp',
    '.DS_Store'
  };

  RepoManager({Directory? rootDir})
      : rootDir = rootDir ??
            Directory(p.join(
                Platform.environment['HOME'] ??
                    Platform.environment['USERPROFILE']!,
                '.hugind'));

  Future<Map<String, String>> _getHeaders() async {
    final token = await GlobalSettings.getHfToken();
    if (token != null && token.isNotEmpty) {
      return {'Authorization': 'Bearer $token'};
    }
    return {};
  }

  /// Lists downloaded repositories in format "user/repo"
  Future<List<String>> listRepos() async {
    if (!await rootDir.exists()) return [];

    final repos = <String>[];

    await for (final userDir in rootDir.list()) {
      if (userDir is Directory) {
        final userName = p.basename(userDir.path);

        // SKIP reserved folders (like 'configs')
        if (_reservedDirs.contains(userName) || userName.startsWith('.')) {
          continue;
        }

        await for (final repoDir in userDir.list()) {
          if (repoDir is Directory) {
            final repoName = p.basename(repoDir.path);
            repos.add('$userName/$repoName');
          }
        }
      }
    }
    return repos;
  }

  Future<List<File>> getLocalFiles(String repo) async {
    final repoDir = _getRepoDir(repo);
    if (!await repoDir.exists()) return [];

    final files = <File>[];
    await for (final entity in repoDir.list()) {
      // Filter out hidden files and temporary .part files
      if (entity is File) {
        final name = p.basename(entity.path);
        if (!name.startsWith('.') && !name.endsWith('.part')) {
          files.add(entity);
        }
      }
    }
    // Sort nicely by name
    files.sort((a, b) => a.path.compareTo(b.path));
    return files;
  }

  Future<List<String>> fetchHFFiles(String repo) async {
    final uri = Uri.parse('https://huggingface.co/api/models/$repo');

    final client = http.Client();
    try {
      final headers = await _getHeaders();
      final response = await client
          .get(uri, headers: headers)
          .timeout(const Duration(seconds: 10));

      if (response.statusCode != 200) {
        throw Exception('Failed to fetch repo info: ${response.statusCode}');
      }

      final json = jsonDecode(response.body);
      final siblings = json['siblings'] as List<dynamic>;

      return siblings
          .map((s) => s['rfilename'] as String)
          .where((f) => f.endsWith('.gguf'))
          .toList();
    } catch (e) {
      throw Exception('Could not connect to Hugging Face: $e');
    } finally {
      client.close();
    }
  }

  Future<void> downloadFile(String repo, String filename,
      {Function(int, int?)? onProgress}) async {
    final url =
        Uri.parse('https://huggingface.co/$repo/resolve/main/$filename');

    final repoDir = _getRepoDir(repo);
    if (!await repoDir.exists()) {
      await repoDir.create(recursive: true);
    }

    // Use a temporary file extension during download
    final finalFile = File(p.join(repoDir.path, filename));
    final tempFile = File(p.join(repoDir.path, '$filename.part'));

    // If a partial download exists, delete it to start fresh (resuming is complex)
    if (await tempFile.exists()) {
      await tempFile.delete();
    }

    final client = http.Client();
    try {
      final request = http.Request('GET', url);
      request.headers.addAll(await _getHeaders());
      final response = await client.send(request);

      if (response.statusCode != 200) {
        throw Exception(
            'Failed to download file: ${response.statusCode} ${response.reasonPhrase}');
      }

      final total = response.contentLength;
      int received = 0;
      final sink = tempFile.openWrite();

      try {
        await response.stream.listen(
          (chunk) {
            sink.add(chunk);
            received += chunk.length;
            if (onProgress != null) {
              onProgress(received, total);
            }
          },
          cancelOnError: true,
        ).asFuture();

        await sink.flush();
        await sink.close();

        // Atomic rename: Only allow the file to exist if download succeeded
        if (await finalFile.exists()) {
          await finalFile.delete();
        }
        await tempFile.rename(finalFile.path);
      } catch (e) {
        await sink.close();
        if (await tempFile.exists()) {
          await tempFile.delete(); // Cleanup corrupted part
        }
        rethrow;
      }
    } finally {
      client.close();
    }
  }

  Future<void> deleteFile(String repo, String filename) async {
    final file = File(p.join(_getRepoDir(repo).path, filename));
    if (await file.exists()) {
      await file.delete();
    }
    _cleanupEmptyDirs(repo);
  }

  Future<bool> repoExists(String repo) async {
    return await _getRepoDir(repo).exists();
  }

  Future<void> deleteRepo(String repo) async {
    final repoDir = _getRepoDir(repo);
    if (await repoDir.exists()) {
      await repoDir.delete(recursive: true);
    }

    // Cleanup parent user directory if empty
    final parent = repoDir.parent;
    if (await parent.exists() && await parent.list().isEmpty) {
      await parent.delete();
    }
  }

  Directory _getRepoDir(String repo) {
    final parts = repo.split('/');
    if (parts.length != 2) {
      throw Exception('Invalid repo format: "$repo". Expected "user/repo".');
    }
    return Directory(p.join(rootDir.path, parts[0], parts[1]));
  }

  Future<void> _cleanupEmptyDirs(String repo) async {
    final repoDir = _getRepoDir(repo);
    if (await repoDir.exists() && await repoDir.list().isEmpty) {
      await repoDir.delete();
      if (await repoDir.parent.list().isEmpty) {
        await repoDir.parent.delete();
      }
    }
  }
}
