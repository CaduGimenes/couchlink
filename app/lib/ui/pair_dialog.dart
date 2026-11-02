import 'dart:async';

import 'package:flutter/material.dart';

import '../l10n/l10n.dart';

import '../daemon/client.dart';
import '../daemon/models.dart';
import '../theme/tokens.dart';
import 'app_state.dart';
import 'icons.dart';
import 'kit.dart';

/// The last tile of the Devices row: pair a new Bluetooth controller. Hidden when this PC has no Bluetooth to pair with.
class PairTile extends StatefulWidget {
  const PairTile({super.key});

  @override
  State<PairTile> createState() => _PairTileState();
}

class _PairTileState extends State<PairTile> {
  bool _available = false;

  @override
  void initState() {
    super.initState();
    WidgetsBinding.instance.addPostFrameCallback((_) async {
      final r = await Scope.of(context).client.bluetoothDevices();
      if (mounted && (r.value?.available ?? false)) setState(() => _available = true);
    });
  }

  @override
  Widget build(BuildContext context) {
    if (!_available) return const SizedBox.shrink();
    final c = context.c;
    return Pressable(
      key: const ValueKey('pair-tile'),
      radius: 12,
      semanticLabel: t('Pair a controller'),
      onTap: () => showPairDialog(context),
      builder: (context, s) => CustomPaint(
        painter: DashedRRectPainter(color: s.hover ? c.accent : c.hairStrong, radius: 12),
        child: Padding(
          padding: const EdgeInsets.fromLTRB(10, 9, 14, 9),
          child: Row(mainAxisSize: MainAxisSize.min, children: [
            SizedBox(width: 30, height: 30, child: Center(child: CIcon(iconPlus, size: 20, color: c.accent, strokeWidth: 2))),
            const SizedBox(width: 10),
            Column(crossAxisAlignment: CrossAxisAlignment.start, mainAxisSize: MainAxisSize.min, children: [
              Text(t('Pair a controller'), style: txt(context, size: 13, weight: FontWeight.w600, height: 1.3)),
              Text(t('Bluetooth'), style: txt(context, size: 11.5, color: c.ink3, height: 1.3)),
            ]),
          ]),
        ),
      ),
    );
  }
}

/// Looks for controllers in pairing mode and pairs the one the player picks.
Future<void> showPairDialog(BuildContext context) {
  return showDialog<void>(context: context, builder: (_) => const _PairDialog());
}

class _PairDialog extends StatefulWidget {
  const _PairDialog();

  @override
  State<_PairDialog> createState() => _PairDialogState();
}

class _PairDialogState extends State<_PairDialog> with SingleTickerProviderStateMixin {
  late final CouchClient _client = Scope.of(context).client;
  late final AnimationController _pulse = AnimationController(vsync: this, duration: const Duration(milliseconds: 900));
  BluetoothPads? _pads;
  String? _error;
  Timer? _poll;

  @override
  void initState() {
    super.initState();
    _pulse.repeat(reverse: true);
    _client.bluetoothScan(true);
    _poll = Timer.periodic(const Duration(milliseconds: 1500), (_) => _load());
    WidgetsBinding.instance.addPostFrameCallback((_) => _load());
  }

  @override
  void dispose() {
    _poll?.cancel();
    _pulse.dispose();
    _client.bluetoothScan(false);
    super.dispose();
  }

  Future<void> _load() async {
    final r = await _client.bluetoothDevices();
    if (!mounted) return;
    setState(() {
      if (r.error != null) {
        _error = r.error;
      } else {
        _pads = r.value;
        _error = null;
      }
    });
  }

  Future<void> _pair(BluetoothPad pad) async {
    final err = await _client.bluetoothPair(pad.id);
    if (!mounted) return;
    if (err != null) setState(() => _error = err);
    await _load();
  }

  Widget _row(BuildContext context, BluetoothPad pad) {
    final c = context.c;
    final sub = pad.kind.isEmpty ? t('Ready to pair') : t('{kind} · ready to pair', {'kind': pad.kind});
    final label = switch (pad.state) {
      'pairing' => t('Pairing…'),
      'paired' => t('Paired'),
      _ => t('Pair'),
    };
    return Container(
      key: ValueKey('pair-row-${pad.id}'),
      margin: const EdgeInsets.only(bottom: 8),
      padding: const EdgeInsets.fromLTRB(10, 9, 12, 9),
      decoration: BoxDecoration(color: c.pane, borderRadius: BorderRadius.circular(12), border: Border.all(color: c.hair)),
      child: Row(children: [
        CIcon(iconPad, size: 30, color: c.ink2),
        const SizedBox(width: 10),
        Expanded(
          child: Column(crossAxisAlignment: CrossAxisAlignment.start, children: [
            Text(pad.name, maxLines: 1, overflow: TextOverflow.ellipsis, style: txt(context, size: 13, weight: FontWeight.w600, height: 1.3)),
            Text(sub, style: txt(context, size: 11.5, color: c.ink3, height: 1.3)),
            if (pad.state == 'failed' && pad.error != null)
              Padding(
                padding: const EdgeInsets.only(top: 2),
                child: Text(t(pad.error!), key: ValueKey('pair-error-${pad.id}'), style: txt(context, size: 11.5, color: c.danger, height: 1.3)),
              ),
          ]),
        ),
        const SizedBox(width: 10),
        CButton(
          key: ValueKey('pair-${pad.id}'),
          label: label,
          small: true,
          primary: pad.state == 'found' || pad.state == 'failed',
          onTap: pad.state == 'found' || pad.state == 'failed' ? () => _pair(pad) : null,
        ),
      ]),
    );
  }

  @override
  Widget build(BuildContext context) {
    final c = context.c;
    final small = txt(context, size: 13, color: c.ink2, height: 1.4);
    Widget how(String who, String what) => Padding(
          padding: const EdgeInsets.only(top: 3),
          child: Text.rich(TextSpan(style: txt(context, size: 12.5, color: c.ink3, height: 1.35), children: [
            TextSpan(text: '$who: ', style: txt(context, size: 12.5, weight: FontWeight.w600, color: c.ink2, height: 1.35)),
            TextSpan(text: what),
          ])),
        );
    final pads = _pads?.devices ?? const <BluetoothPad>[];
    return Dialog(
      backgroundColor: c.window,
      shape: RoundedRectangleBorder(borderRadius: BorderRadius.circular(18), side: BorderSide(color: c.hairStrong)),
      child: ConstrainedBox(
        constraints: const BoxConstraints(maxWidth: 460, maxHeight: 620),
        child: Padding(
          padding: const EdgeInsets.all(22),
          child: Column(mainAxisSize: MainAxisSize.min, crossAxisAlignment: CrossAxisAlignment.start, children: [
            Text(t('Pair a controller'), style: txt(context, size: 17, weight: FontWeight.w700)),
            const SizedBox(height: 12),
            Flexible(
              child: SingleChildScrollView(
                child: Column(crossAxisAlignment: CrossAxisAlignment.start, children: [
                  Text(t('Put the controller in pairing mode. It blinks quickly:'), style: small),
                  how(t('PlayStation'), t('hold Share (Create) and PS')),
                  how(t('Xbox'), t('hold the pairing button on top')),
                  how(t('Switch Pro'), t('hold the button on top')),
                  const SizedBox(height: 12),
                  Text(t('Pick it below.'), style: small),
                  const SizedBox(height: 10),
                  for (final pad in pads) _row(context, pad),
                  if (_pads?.scanning ?? true)
                    Padding(
                      padding: const EdgeInsets.symmetric(vertical: 6),
                      child: Row(children: [
                        FadeTransition(
                          opacity: Tween<double>(begin: .3, end: 1).animate(_pulse),
                          child: Container(key: const ValueKey('pair-scanning'), width: 8, height: 8, decoration: BoxDecoration(color: c.accent, shape: BoxShape.circle)),
                        ),
                        const SizedBox(width: 10),
                        Expanded(child: Text(t('Looking for controllers in pairing mode…'), style: txt(context, size: 12.5, color: c.ink3))),
                      ]),
                    ),
                  if (_error != null) Padding(padding: const EdgeInsets.only(top: 6), child: Text(_error!, style: txt(context, size: 12.5, color: c.danger))),
                  const SizedBox(height: 10),
                  Text(t('Xbox controllers on Microsoft’s USB wireless adapter pair with the adapter’s own button instead.'), style: txt(context, size: 12, color: c.ink3, height: 1.4)),
                ]),
              ),
            ),
            const SizedBox(height: 16),
            Align(
              alignment: Alignment.centerRight,
              child: CButton(key: const ValueKey('pair-done'), label: t('Done'), small: true, onTap: () => Navigator.of(context).pop()),
            ),
          ]),
        ),
      ),
    );
  }
}
