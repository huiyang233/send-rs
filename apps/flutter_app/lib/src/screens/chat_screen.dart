import 'package:flutter/material.dart';

class ChatScreen extends StatelessWidget {
  const ChatScreen({super.key});

  @override
  Widget build(BuildContext context) {
    return SafeArea(
      child: Padding(
        padding: const EdgeInsets.all(16),
        child: Column(
          crossAxisAlignment: CrossAxisAlignment.start,
          children: [
            Text('Chat', style: Theme.of(context).textTheme.headlineSmall),
            const SizedBox(height: 12),
            const Expanded(
              child: Card(
                child: Center(
                  child: Text('Select a paired peer to chat'),
                ),
              ),
            ),
            Row(
              children: [
                const Expanded(
                  child: TextField(
                    decoration: InputDecoration(
                      hintText: 'Type a message',
                      border: OutlineInputBorder(),
                    ),
                  ),
                ),
                const SizedBox(width: 8),
                FilledButton(onPressed: null, child: const Text('Send')),
              ],
            ),
          ],
        ),
      ),
    );
  }
}
