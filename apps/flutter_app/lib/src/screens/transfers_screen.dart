import 'package:flutter/material.dart';

class TransfersScreen extends StatelessWidget {
  const TransfersScreen({super.key});

  @override
  Widget build(BuildContext context) {
    return SafeArea(
      child: Padding(
        padding: const EdgeInsets.all(16),
        child: Column(
          crossAxisAlignment: CrossAxisAlignment.start,
          children: [
            Row(
              children: [
                Text('Transfer Queue', style: Theme.of(context).textTheme.headlineSmall),
                const Spacer(),
                SegmentedButton<bool>(
                  segments: const [
                    ButtonSegment(value: false, label: Text('LAN')), 
                    ButtonSegment(value: true, label: Text('Public')),
                  ],
                  selected: const {false},
                  onSelectionChanged: (_) {},
                ),
              ],
            ),
            const SizedBox(height: 12),
            const Card(
              child: ListTile(
                title: Text('No active transfer'),
                subtitle: Text('Use Devices tab to start sending files/folders'),
              ),
            ),
          ],
        ),
      ),
    );
  }
}
