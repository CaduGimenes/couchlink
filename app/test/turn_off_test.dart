import 'package:couchlink/daemon/models.dart';
import 'package:couchlink/ui/app_state.dart';
import 'package:flutter/gestures.dart';
import 'package:flutter_test/flutter_test.dart';

import 'helpers.dart';

void main() {
  testWidgets('a wireless controller has a power button that turns it off; a wired one has none', (tester) async {
    final fake = newFake();
    await pumpApp(tester, fake);
    expect(byKey('turn-off-pad:1'), findsOneWidget);
    expect(byKey('turn-off-pad:3'), findsOneWidget);
    expect(byKey('turn-off-pad:2'), findsNothing, reason: 'the DualSense is wired');
    expect(byKey('turn-off-kbm:system'), findsNothing);

    await tester.tap(byKey('turn-off-pad:3'));
    await tester.pump(const Duration(milliseconds: 100));
    expect(fake.turnedOff, ['pad:3']);
    expect(byKey('device-pad:3'), findsNothing, reason: 'the controller drops off the list');
    expect(byKey('turn-off-pad:1'), findsOneWidget);
  });

  testWidgets('the power button shows its tooltip', (tester) async {
    await pumpApp(tester, newFake());
    final g = await tester.createGesture(kind: PointerDeviceKind.mouse);
    await g.addPointer(location: Offset.zero);
    addTearDown(g.removePointer);
    await g.moveTo(tester.getCenter(byKey('turn-off-pad:1')));
    await tester.pump(const Duration(milliseconds: 600));
    expect(find.text('Turn off'), findsOneWidget);
  });

  test('the daemon refuses a controller that cannot be turned off, and the fake says so', () async {
    final fake = newFake();
    expect(await fake.disconnectDevice('pad:2'), contains('cannot be turned off'));
    expect(await fake.disconnectDevice('pad:99'), isNotNull);
    expect(fake.turnedOff, isEmpty);
  });

  testWidgets('Settings picks the turn-off buttons, which may not be the close-the-games buttons', (tester) async {
    final fake = newFake();
    await pumpApp(tester, fake, view: AppView.library);
    await tester.tap(byKey('settings-open'));
    await tester.pump(const Duration(milliseconds: 200));
    await tester.tap(byKey('settings-tab-1'));
    await tester.pump(const Duration(milliseconds: 100));
    expect((fake.state as DaemonState).turnOffPad, 'off');
    expect((fake.state as DaemonState).stopPad, 'guide+back');

    await tester.ensureVisible(byKey('turn-off-pad-guide+start'));
    await tester.tap(byKey('turn-off-pad-guide+start'));
    await tester.pump(const Duration(milliseconds: 100));
    expect((fake.state as DaemonState).turnOffPad, 'guide+start');

    // The close-the-games buttons are taken.
    await tester.ensureVisible(byKey('turn-off-pad-guide+back'));
    await tester.tap(byKey('turn-off-pad-guide+back'));
    await tester.pump(const Duration(milliseconds: 100));
    expect((fake.state as DaemonState).turnOffPad, 'guide+start');
    expect(find.textContaining('already closes the games'), findsOneWidget);
    await tester.pump(const Duration(seconds: 8));

    await tester.ensureVisible(byKey('turn-off-pad-off'));
    await tester.tap(byKey('turn-off-pad-off'));
    await tester.pump(const Duration(milliseconds: 100));
    expect((fake.state as DaemonState).turnOffPad, 'off');
  });

  test('the state carries can_turn_off and turn_off_pad, with safe defaults', () {
    final st = DaemonState.fromJson({
      'devices': [
        {'id': 'pad:1', 'kind': 'gamepad', 'name': 'DualShock 4', 'family': 'playstation', 'connection': 'wireless', 'can_turn_off': true},
        {'id': 'pad:2', 'kind': 'gamepad', 'name': 'Old', 'family': 'generic', 'connection': 'usb'},
      ],
      'turn_off_pad': 'back+start',
    });
    expect(st.devices[0].canTurnOff, isTrue);
    expect(st.devices[1].canTurnOff, isFalse);
    expect(st.turnOffPad, 'back+start');
    expect(DaemonState.fromJson({}).turnOffPad, 'off');
  });
}
