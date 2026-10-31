import 'package:flutter/material.dart';
import 'package:flutter/services.dart' show Clipboard, ClipboardData;

import '../l10n/l10n.dart';
import '../theme/tokens.dart';
import 'app_state.dart';
import 'icons.dart';
import 'kit.dart';
import 'shell.dart';

/// The prompt a player pastes into their AI coding agent to make a recipe for a game Couchlink does not know yet.
/// It stays in English: the docs it points to are English, and agents follow it the same in any language.
const recipePrompt = '''Help me make a Couchlink recipe so a PC game runs split-screen on one PC.

Couchlink (https://github.com/cranebeaker/couchlink) runs one copy of a game per player and gives each copy its own controller or keyboard and mouse. A recipe is one JSON file that tells it how.

1. Read these first:
   - https://raw.githubusercontent.com/cranebeaker/couchlink/main/skills/couchlink-sdk/SKILL.md (section 1, "Write a recipe")
   - https://raw.githubusercontent.com/cranebeaker/couchlink/main/docs/launch.md (section "Recipe", every field)
   - Examples: https://github.com/cranebeaker/couchlink/tree/main/core/recipes
2. Ask me which game it is, then find out on my PC:
   - where it is installed and which exe starts it (and its Steam app id if it is a Steam game)
   - how players connect: local co-op, LAN or direct IP, a dedicated server, or only through Steam
   - command line options for windowed mode, resolution and player name
   - where it keeps saves and settings (so each player gets their own)
   - the mutex or event it uses to refuse a second copy, if it does
3. Write the recipe to %LOCALAPPDATA%\\Couchlink\\recipes\\<game-id>.json. Keep it to what the format supports: data only, no scripts.
4. Check it parses. With the Couchlink source: cargo run -q -p couchlink-nucleus --example check_recipe -- <file>. Otherwise re-read launch.md and check every field by hand.
5. Tell me to open Couchlink: the game shows up in the Library. If it does not start right, read the copy logs in %LOCALAPPDATA%\\Couchlink\\logs and fix the recipe with me.

When it works, I can share it in the Co-op Anything Discord so everyone gets it.''';

class RecipePromptView extends StatelessWidget {
  const RecipePromptView({super.key});

  @override
  Widget build(BuildContext context) {
    final c = context.c;
    final toaster = Scope.of(context).toaster;
    Future<void> copy() async {
      await Clipboard.setData(const ClipboardData(text: recipePrompt));
      toaster.show(t('Copied. Paste it into your AI coding agent.'));
    }

    return ViewFrame(
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          PageHead(
            title: t('Make your own recipe'),
            subtitle: t('A game Couchlink does not know yet? Copy this prompt into an AI coding agent (Claude Code, Codex, Cursor and the like). It reads the recipe docs, looks at the game on your PC and writes the recipe with you.'),
            trailing: CButton(
              key: const ValueKey('copy-recipe-prompt'),
              label: t('Copy prompt'),
              small: true,
              primary: true,
              leading: CIcon(iconCopy, size: 14, color: c.accentInk, strokeWidth: 1.8),
              onTap: copy,
            ),
          ),
          Panel(
            radius: 16,
            padding: const EdgeInsets.all(18),
            child: Column(
              crossAxisAlignment: CrossAxisAlignment.start,
              children: [
                Text(t('The prompt'), style: txt(context, size: 15, weight: FontWeight.w700, ls: -.01, height: 1.3)),
                const SizedBox(height: 10),
                Container(
                  width: double.infinity,
                  padding: const EdgeInsets.symmetric(horizontal: 14, vertical: 12),
                  decoration: BoxDecoration(color: c.ground, borderRadius: BorderRadius.circular(10), border: Border.all(color: c.hair)),
                  child: SelectableText(
                    recipePrompt,
                    key: const ValueKey('recipe-prompt'),
                    style: txt(context, size: 12, color: c.ink2, height: 1.55).copyWith(fontFamily: monoFamily, fontFamilyFallback: monoFallback),
                  ),
                ),
                const SizedBox(height: 10),
                Text(
                  t('Your recipe goes in %LOCALAPPDATA%\\Couchlink\\recipes and shows up in the Library right away. Nothing is shared until you post it.'),
                  style: txt(context, size: 12, color: c.ink3, height: 1.4),
                ),
              ],
            ),
          ),
        ],
      ),
    );
  }
}
