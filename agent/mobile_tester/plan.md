use mobile_launch_app to launch the app on the device
for example google keep app
then add new noew with title "test" and body "test" using tools

Device Management
mobile_list_available_devices - List all available devices (simulators, emulators, and real devices)
mobile_get_screen_size - Get the screen size of the mobile device in pixels
mobile_get_orientation - Get the current screen orientation of the device
mobile_set_orientation - Change the screen orientation (portrait/landscape)
App Management
mobile_list_apps - List all installed apps on the device
mobile_launch_app - Launch an app using its package name
mobile_terminate_app - Stop and terminate a running app
mobile_install_app - Install an app from file (.apk, .ipa, .app, .zip)
mobile_uninstall_app - Uninstall an app using bundle ID or package name
Screen Interaction
mobile_take_screenshot - Take a screenshot to understand what's on screen
mobile_save_screenshot - Save a screenshot to a file
mobile_list_elements_on_screen - List UI elements with their coordinates and properties
mobile_click_on_screen_at_coordinates - Click at specific x,y coordinates
mobile_double_tap_on_screen - Double-tap at specific coordinates
mobile_long_press_on_screen_at_coordinates - Long press at specific coordinates
mobile_swipe_on_screen - Swipe in any direction (up, down, left, right)
Input & Navigation
mobile_type_keys - Type text into focused elements with optional submit
mobile_press_button - Press device buttons (HOME, BACK, VOLUME_UP/DOWN, ENTER, etc.)
mobile_open_url - Open URLs in the device browser

use llmchat to get help figuring what to do (aka react loop)