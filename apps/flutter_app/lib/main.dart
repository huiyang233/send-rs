import 'package:flutter/material.dart';

import 'src/screens/chat_screen.dart';
import 'src/screens/devices_screen.dart';
import 'src/screens/settings_screen.dart';
import 'src/screens/transfers_screen.dart';

void main() {
  runApp(const SendRsApp());
}

class SendRsApp extends StatelessWidget {
  const SendRsApp({super.key});

  @override
  Widget build(BuildContext context) {
    return MaterialApp(
      title: 'Send-RS',
      debugShowCheckedModeBanner: false,
      theme: ThemeData(
        colorScheme: ColorScheme.fromSeed(seedColor: const Color(0xFF005F73)),
        useMaterial3: true,
      ),
      home: const HomeShell(),
    );
  }
}

class HomeShell extends StatefulWidget {
  const HomeShell({super.key});

  @override
  State<HomeShell> createState() => _HomeShellState();
}

class _HomeShellState extends State<HomeShell> {
  int _selected = 0;

  final _pages = const [
    DevicesScreen(),
    TransfersScreen(),
    ChatScreen(),
    SettingsScreen(),
  ];

  @override
  Widget build(BuildContext context) {
    return Scaffold(
      body: _pages[_selected],
      bottomNavigationBar: NavigationBar(
        selectedIndex: _selected,
        onDestinationSelected: (index) {
          setState(() => _selected = index);
        },
        destinations: const [
          NavigationDestination(icon: Icon(Icons.devices), label: 'Devices'),
          NavigationDestination(icon: Icon(Icons.folder_zip), label: 'Transfers'),
          NavigationDestination(icon: Icon(Icons.chat_bubble_outline), label: 'Chat'),
          NavigationDestination(icon: Icon(Icons.settings), label: 'Settings'),
        ],
      ),
    );
  }
}
