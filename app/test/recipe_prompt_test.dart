import 'package:couchlink/ui/app_state.dart';
import 'package:couchlink/ui/recipe_prompt_view.dart';
import 'package:flutter/services.dart';
import 'package:flutter_test/flutter_test.dart';

import 'helpers.dart';

void main() {
  testWidgets('Make your own recipe shows the prompt and copies it', (tester) async {
    String? copied;
    tester.binding.defaultBinaryMessenger.setMockMethodCallHandler(SystemChannels.platform, (call) async {
      if (call.method == 'Clipboard.setData') copied = (call.arguments as Map)['text'] as String;
      return null;
    });
    addTearDown(() => tester.binding.defaultBinaryMessenger.setMockMethodCallHandler(SystemChannels.platform, null));

    await pumpApp(tester, newFake(), view: AppView.library);
    await tester.tap(byKey('nav-recipe'));
    await tester.pump(const Duration(milliseconds: 400));
    expect(byKey('recipe-prompt'), findsOneWidget);

    await tester.tap(byKey('copy-recipe-prompt'));
    await tester.pump(const Duration(milliseconds: 100));
    expect(copied, recipePrompt);
    expect(recipePrompt, contains('skills/couchlink-sdk/SKILL.md'));
    expect(recipePrompt, contains('docs/launch.md'));
  });
}
