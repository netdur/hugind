import 'package:args/command_runner.dart';
import 'package:interact/interact.dart';
import 'package:path/path.dart' as p;
import '../repo_manager.dart';

class ModelCommand extends Command {
  @override
  final String name = 'model';
  @override
  final String description = 'Manage AI models (download, list, remove).';

  ModelCommand({RepoManager? manager}) {
    addSubcommand(ListCommand(manager: manager));
    addSubcommand(AddCommand(manager: manager));
    addSubcommand(RemoveCommand(manager: manager));
    addSubcommand(ShowCommand(manager: manager));
  }
}

class ListCommand extends Command {
  @override
  final String name = 'list';
  @override
  final String description = 'List all downloaded model repositories.';

  final RepoManager? _manager;
  ListCommand({RepoManager? manager}) : _manager = manager;

  @override
  Future<void> run() async {
    final manager = _manager ?? RepoManager();
    final repos = await manager.listRepos();

    if (repos.isEmpty) {
      print(
          'No models found. Run "hugind model add <hf_repo>" to download one.');
      return;
    }

    print('\nDownloaded Repositories:');
    print('-' * 40);
    for (var repo in repos) {
      print(repo);
    }
    print('');
  }
}

class AddCommand extends Command {
  @override
  final String name = 'add';
  @override
  final String description = 'Download GGUF files from Hugging Face.';

  final RepoManager? _manager;
  AddCommand({RepoManager? manager}) : _manager = manager;

  @override
  Future<void> run() async {
    if (argResults!.rest.isEmpty) {
      print('Usage: hugind model add <user/repo>');
      print('Example: hugind model add TheBloke/Llama-2-7B-Chat-GGUF');
      return;
    }

    final repo = argResults!.rest.first;
    final manager = _manager ?? RepoManager();

    // 1. Fetch List
    final spinner = Spinner(
      icon: '🔍',
      leftPrompt: (done) =>
          done ? 'Fetched file list' : 'Scanning $repo for GGUF files...',
    ).interact();

    List<String> files;
    try {
      files = await manager.fetchHFFiles(repo);
    } catch (e) {
      spinner.done();
      print('❌ Error fetching files: $e');
      return;
    }
    spinner.done();

    if (files.isEmpty) {
      print('No GGUF files found in $repo.');
      return;
    }

    // 2. Select Files
    final selection = MultiSelect(
      prompt: 'Select files to download (Space to select, Enter to confirm):',
      options: files,
    ).interact();

    if (selection.isEmpty) {
      print('No files selected.');
      return;
    }

    // 3. Download Loop
    print('\nStarting download for ${selection.length} file(s)...');

    for (var index in selection) {
      final filename = files[index];

      final progress = Progress(
        length: 100,
        leftPrompt: (c) => 'Downloading $filename',
        rightPrompt: (c) => '${c}%',
      ).interact();

      int lastPercent = 0;
      try {
        await manager.downloadFile(repo, filename,
            onProgress: (received, total) {
          if (total != null && total > 0) {
            final currentPercent = (received / total * 100).floor();
            if (currentPercent > lastPercent) {
              progress.increase(currentPercent - lastPercent);
              lastPercent = currentPercent;
            }
          }
        });
        progress.done();
        // print('✅ Saved to: ~/.hugind/$repo/$filename');
      } catch (e) {
        progress.done();
        print('❌ Error downloading $filename: $e');
      }
    }
    print('\nDone.');
  }
}

class RemoveCommand extends Command {
  @override
  final String name = 'remove';
  @override
  final String description = 'Delete files or repositories.';

  final RepoManager? _manager;
  RemoveCommand({RepoManager? manager}) : _manager = manager;

  @override
  Future<void> run() async {
    if (argResults!.rest.isEmpty) {
      print('Usage: hugind model remove <user/repo>');
      return;
    }

    final repo = argResults!.rest.first;
    final manager = _manager ?? RepoManager();

    try {
      if (!await manager.repoExists(repo)) {
        print('Repository "$repo" does not exist locally.');
        return;
      }

      final files = await manager.getLocalFiles(repo);

      // Case A: Repo exists but has no files (empty folder)
      if (files.isEmpty) {
        if (Confirm(
                prompt: 'Repository is empty. Delete folder?',
                defaultValue: true)
            .interact()) {
          await manager.deleteRepo(repo);
          print('🗑️  Deleted $repo');
        }
        return;
      }

      // Case B: Repo has files - Select to delete
      final fileNames = files.map((f) => p.basename(f.path)).toList();

      // Add option to delete everything
      final options = ['[DELETE ENTIRE REPO]', ...fileNames];

      final selection = MultiSelect(
        prompt: 'Select items to delete:',
        options: options,
      ).interact();

      if (selection.isEmpty) return;

      // Check if "Delete Repo" was selected (index 0)
      if (selection.contains(0)) {
        if (Confirm(
                prompt: 'Are you sure you want to delete the entire "$repo"?')
            .interact()) {
          await manager.deleteRepo(repo);
          print('🗑️  Deleted repository $repo');
        }
        return;
      }

      // Delete selected files
      for (var index in selection) {
        final filename = options[index]; // options map 1:1 to index
        await manager.deleteFile(repo, filename);
        print('🗑️  Deleted $filename');
      }

      // Cleanup empty repo check
      if ((await manager.getLocalFiles(repo)).isEmpty) {
        if (Confirm(
                prompt: 'Repository is now empty. Delete folder?',
                defaultValue: true)
            .interact()) {
          await manager.deleteRepo(repo);
          print('🗑️  Cleaned up empty folder.');
        }
      }
    } catch (e) {
      print('Error: $e');
    }
  }
}

class ShowCommand extends Command {
  @override
  final String name = 'show';
  @override
  final String description = 'List local files in a repository.';

  final RepoManager? _manager;
  ShowCommand({RepoManager? manager}) : _manager = manager;

  @override
  Future<void> run() async {
    if (argResults!.rest.isEmpty) {
      print('Usage: hugind model show <user/repo>');
      return;
    }

    final repo = argResults!.rest.first;
    final manager = _manager ?? RepoManager();

    if (!await manager.repoExists(repo)) {
      print('Repository "$repo" not found locally.');
      return;
    }

    final files = await manager.getLocalFiles(repo);
    if (files.isEmpty) {
      print('Repository is empty.');
      return;
    }

    print('\nFiles in $repo:');
    print('-' * 40);
    for (var file in files) {
      final name = p.basename(file.path);
      final sizeMb = (file.lengthSync() / (1024 * 1024)).toStringAsFixed(2);
      print('$name  (${sizeMb} MB)');
    }
    print('');
  }
}
