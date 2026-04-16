import 'package:flutter/material.dart';

class DevicesScreen extends StatelessWidget {
  const DevicesScreen({super.key});

  @override
  Widget build(BuildContext context) {
    return SafeArea(
      child: Padding(
        padding: const EdgeInsets.all(16),
        child: Column(
          crossAxisAlignment: CrossAxisAlignment.start,
          children: [
            Text('Nearby Devices', style: Theme.of(context).textTheme.headlineSmall),
            const SizedBox(height: 12),
            const Card(
              child: ListTile(
                leading: Icon(Icons.phone_android),
                title: Text('No peer discovered yet'),
                subtitle: Text('Tap discover to broadcast on LAN'),
              ),
            ),
            const Spacer(),
            FilledButton.icon(
              onPressed: () {},
              icon: const Icon(Icons.radar),
              label: const Text('Discover'),
            ),
          ],
        ),
      ),
    );
  }
}
