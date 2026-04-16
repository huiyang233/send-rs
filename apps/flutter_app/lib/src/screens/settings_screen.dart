import 'package:flutter/material.dart';

class SettingsScreen extends StatelessWidget {
  const SettingsScreen({super.key});

  @override
  Widget build(BuildContext context) {
    return SafeArea(
      child: ListView(
        padding: const EdgeInsets.all(16),
        children: const [
          ListTile(
            leading: Icon(Icons.security),
            title: Text('Pairing and Trust'),
            subtitle: Text('Manage trusted devices and pairing codes'),
          ),
          ListTile(
            leading: Icon(Icons.storage),
            title: Text('Chat Retention'),
            subtitle: Text('Messages are stored locally for long-term history'),
          ),
          ListTile(
            leading: Icon(Icons.public),
            title: Text('Public Network Mode'),
            subtitle: Text('Disabled by default. Enable per transfer when needed.'),
          ),
        ],
      ),
    );
  }
}
