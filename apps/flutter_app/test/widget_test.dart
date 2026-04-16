import 'package:flutter_test/flutter_test.dart';
import 'package:sendrs_app/main.dart';

void main() {
  testWidgets('app boots', (tester) async {
    await tester.pumpWidget(const SendRsApp());
    expect(find.text('Devices'), findsOneWidget);
  });
}
