trait Greetable {
    fn greet(&self) -> String;
    fn name(&self) -> &str;
}

trait Farewellable {
    fn farewell(&self) -> String;

    fn wave(&self) -> String {
        format!("*waves at {}*", self.farewell())
    }
}
